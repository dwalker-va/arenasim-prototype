//! AS-218 — under `TeamPlan` the Druid holds its damage and utility while the
//! team solve walks it to a dying teammate it cannot yet reach, as it does
//! for the `Legacy` medic walk (AS-210).
//!
//! `TeamPlan` retires the medic chase in PRESSURED: there the team solve is
//! the one positioning authority, and it walks the healer to the dying
//! teammate itself — reach of it ranks above cover (`C_DYING`), and when no
//! spot the solve weighs can see it the solve walks straight at it (the sight
//! fallback). The hold read only the `Legacy` walk, so under `TeamPlan` the
//! Druid spent the global cooldown on a Moonfire while the solve walked it
//! into reach. The solve's teammate now reaches the hold through the posture
//! plan (`CasterHealerPlan::medic_walk`, from
//! `healer_postures::solve_dying_walk`).
//!
//! Scenes run the whole decision — the posture machine and the rotation —
//! through `dispatch_class_decision`, the entry point `decide_abilities`
//! runs. Every scene is a pair: the walk, and the same scene with its one
//! condition removed.

use arenasim::states::match_config::CharacterClass::{self, Druid, Mage, Shaman, Warrior};
use arenasim::states::play_match::ai_profile::AiProfile;
use arenasim::states::play_match::class_ai::scene::{DecisionView, Scene};
use arenasim::states::play_match::components::{
    Aura, AuraType, CompoundDebuff, HealerPosture, MovementDirective, MovementGoal, Posture,
};
use arenasim::states::play_match::decision_trace::RejectionReason;
use arenasim::states::play_match::map_geometry::ObstacleVolume;
use arenasim::states::play_match::{AbilityType, SpellSchool};
use bevy::prelude::{Vec2, Vec3};

const DRUID: &str = "druid";
const WARRIOR: &str = "dying warrior";
const ENEMY: &str = "enemy";
const SHAMAN: &str = "shaman";

/// Below `urgency_hp_threshold` (0.5), above `DRUID_EMERGENCY_HP` (0.45).
const DYING: f32 = 0.47;
/// Above `urgency_hp_threshold`: hurt, not dying.
const HURT: f32 = 0.6;

/// The scene clock.
const NOW: f32 = 30.0;

/// Where the Warrior stands: 30yd east of the Druid, in heal range, with
/// [`pillar`] between; or 45yd east, in sight and beyond the 40yd heal range.
const BEHIND_PILLAR: (f32, f32) = (30.0, 0.0);
const OUT_OF_RANGE: (f32, f32) = (45.0, 0.0);

/// A 6yd pillar at (23, 0), squarely between the Druid at the origin and the
/// Warrior [`BEHIND_PILLAR`], and wide enough that none of the solve's own
/// spots — its ring round the Druid, its steps along the line to the Warrior
/// — sees past it. The enemy at (0, 25) sees the Druid.
fn pillar() -> Vec<ObstacleVolume> {
    vec![ObstacleVolume::Cylinder {
        center_xz: Vec2::new(23.0, 0.0),
        radius: 6.0,
        base_y: 0.0,
        height: 10.0,
    }]
}

fn mark_of_the_wild() -> Aura {
    Aura {
        effect_type: AuraType::MaxHealthIncrease,
        compound: Some(CompoundDebuff::MarkOfTheWild),
        duration: 1800.0,
        break_on_damage_threshold: -1.0,
        ability_name: "Mark of the Wild".into(),
        ..Default::default()
    }
}

/// The Druid in PRESSURED, held there by its hysteresis floor.
fn pressured() -> HealerPosture {
    HealerPosture {
        posture: Posture::Pressured,
        hold_until: NOW + 10.0,
        ..HealerPosture::new(NOW - 1.0)
    }
}

/// [`scene_vs`] against an enemy Mage, the Druid in [`pressured`].
fn scene(
    profile: AiProfile,
    warrior: (f32, f32),
    warrior_hp: f32,
    obstacles: Vec<ObstacleVolume>,
) -> Scene {
    scene_vs(Mage, profile, warrior, warrior_hp, obstacles, pressured())
}

/// A Druid with `posture` on team 1 under `profile` — with nobody on it, so
/// nothing calls for Travel Form — its teammate Warrior at `warrior` and
/// `warrior_hp`, and one enemy of class `enemy` at (0, 25), in sight and in
/// Moonfire's range as the Druid's kill target, attacking nobody. Both allies
/// carry Mark of the Wild, so the Mark does not take the turn: with nothing it
/// can heal, the Druid's turn is Moonfire.
fn scene_vs(
    enemy: CharacterClass,
    profile: AiProfile,
    warrior: (f32, f32),
    warrior_hp: f32,
    obstacles: Vec<ObstacleVolume>,
    posture: HealerPosture,
) -> Scene {
    Scene::new()
        .now(NOW)
        .ai_profile(1, profile)
        .unit(DRUID, 1, Druid)
        .targeting(ENEMY)
        .unit(WARRIOR, 1, Warrior)
        .at(warrior.0, warrior.1)
        .health(warrior_hp)
        .unit(ENEMY, 2, enemy)
        .at(0.0, 25.0)
        .aura_full(DRUID, mark_of_the_wild())
        .aura_full(WARRIOR, mark_of_the_wild())
        .healer_posture(DRUID, posture)
        .obstacles(obstacles)
}

fn describe(view: &DecisionView) -> String {
    format!(
        "chose {:?}; candidates {:?}; directive {:?}",
        view.chosen,
        view.candidates
            .iter()
            .map(|c| (c.ability, c.status, c.reason.clone()))
            .collect::<Vec<_>>(),
        view.directive.map(|d| d.goal)
    )
}

/// Moonfire was held for the walk: rejected with the hold's note, and
/// nothing cast.
fn held(view: &DecisionView) -> bool {
    view.chose().is_none()
        && matches!(
            view.rejected(AbilityType::Moonfire),
            Some(RejectionReason::PreconditionUnmet { note }) if note.contains("medic walk")
        )
}

fn moonfired(view: &DecisionView) -> bool {
    view.chose() == Some((AbilityType::Moonfire, Some(ENEMY)))
}

/// The sight fallback: the dying Warrior is in heal range behind the pillar,
/// and no spot the solve weighs sees it, so the solve walks straight at it.
/// The Druid holds Moonfire for the walk. Without the pillar the Warrior is
/// in reach, and gets the heal the hold kept the global cooldown for.
///
/// The enemy is a melee Warrior standing off: a caster would give the solve
/// the pillar's shadow from it to weigh too, and a spot there sees round the
/// pillar — the walk would be `C_DYING`'s, which the next test holds.
#[test]
fn the_solve_walk_round_a_pillar_holds_damage_until_its_teammate_is_reached() {
    let behind = |obstacles| {
        scene_vs(
            Warrior,
            AiProfile::TeamPlan,
            BEHIND_PILLAR,
            DYING,
            obstacles,
            pressured(),
        )
        .decide(DRUID)
    };

    let walk = behind(pillar());
    assert!(
        matches!(
            walk.directive.map(|d| d.goal),
            Some(MovementGoal::Point(p))
                if Vec2::new(p.x, p.z).distance(Vec2::new(BEHIND_PILLAR.0, BEHIND_PILLAR.1)) < 0.5
        ),
        "the solve walks straight at the dying Warrior: {}",
        describe(&walk)
    );
    assert!(held(&walk), "Moonfire held for the walk: {}", describe(&walk));

    let reached = behind(Vec::new());
    assert_eq!(
        reached.chose(),
        Some((AbilityType::Rejuvenation, Some(WARRIOR))),
        "the Warrior in reach: {}",
        describe(&reached)
    );
}

/// `C_DYING`: the dying Warrior is in sight 45yd off, beyond heal range, and
/// the solve walks the Druid into reach of it. The Druid holds Moonfire for
/// the walk. A Warrior that is hurt but not dying owes no walk, and the Druid
/// Moonfires.
#[test]
fn the_solve_walk_into_heal_range_holds_damage() {
    let walk = scene(AiProfile::TeamPlan, OUT_OF_RANGE, DYING, Vec::new()).decide(DRUID);
    assert!(
        matches!(
            walk.directive.map(|d| d.goal),
            Some(MovementGoal::Point(p)) if p.x > 0.5
        ),
        "the solve steps toward the Warrior: {}",
        describe(&walk)
    );
    assert!(held(&walk), "Moonfire held for the walk: {}", describe(&walk));

    let not_dying = scene(AiProfile::TeamPlan, OUT_OF_RANGE, HURT, Vec::new()).decide(DRUID);
    assert!(moonfired(&not_dying), "nobody dying: {}", describe(&not_dying));
}

/// The walk is `TeamPlan`'s: `Legacy` has no walk into heal range (its medic
/// walk is for a teammate behind cover), so the same scene Moonfires — the
/// `Legacy` decision this change leaves alone.
#[test]
fn legacy_has_no_walk_into_heal_range() {
    let view = scene(AiProfile::Legacy, OUT_OF_RANGE, DYING, Vec::new()).decide(DRUID);
    assert!(moonfired(&view), "Legacy: {}", describe(&view));
}

/// The CC gate: a rooted Druid walks nowhere, so there is no walk to hold
/// for — the `Legacy` walk's own gate. It Moonfires. Unrooted, the same scene
/// holds.
#[test]
fn a_rooted_druid_holds_nothing_for_the_walk() {
    let rooted = scene(AiProfile::TeamPlan, OUT_OF_RANGE, DYING, Vec::new())
        .aura(DRUID, AuraType::Root, 4.0)
        .decide(DRUID);
    assert!(moonfired(&rooted), "rooted: {}", describe(&rooted));

    let free = scene(AiProfile::TeamPlan, OUT_OF_RANGE, DYING, Vec::new()).decide(DRUID);
    assert!(held(&free), "not rooted: {}", describe(&free));
}

/// The walk is the solve's, and the solve runs only in PRESSURED: a Druid in
/// FREE carrying a walk left over from an earlier PRESSURED stint holds
/// nothing for it. Carried into a committed PRESSURED tick — one that does
/// not re-solve — the same walk holds, until its teammate is no longer dying.
#[test]
fn a_walk_left_from_an_earlier_pressured_stint_holds_nothing() {
    let probe = scene(AiProfile::TeamPlan, OUT_OF_RANGE, DYING, Vec::new());
    let warrior = probe.entity(WARRIOR);
    let carried = |posture: Posture| HealerPosture {
        posture,
        solve_dying: Some(warrior),
        ..pressured()
    };
    // A committed directive keeps the PRESSURED tick from re-solving, so the
    // walk it reads is the one the posture carries in.
    let committed = MovementDirective {
        goal: MovementGoal::Point(Vec3::new(1.0, 1.0, 0.0)),
        expires: NOW + 1.0,
        committed_until: NOW + 0.5,
    };
    let decide = |warrior_hp: f32, posture: HealerPosture, directive: Option<MovementDirective>| {
        let s = scene_vs(
            Mage,
            AiProfile::TeamPlan,
            OUT_OF_RANGE,
            warrior_hp,
            Vec::new(),
            posture,
        );
        match directive {
            Some(d) => s.directive(DRUID, d),
            None => s,
        }
        .decide(DRUID)
    };

    let free = decide(
        DYING,
        HealerPosture {
            hold_until: 0.0,
            ..carried(Posture::Free)
        },
        None,
    );
    assert!(moonfired(&free), "FREE: {}", describe(&free));

    let pressured = decide(DYING, carried(Posture::Pressured), Some(committed));
    assert!(held(&pressured), "PRESSURED: {}", describe(&pressured));

    let recovered = decide(HURT, carried(Posture::Pressured), Some(committed));
    assert!(
        moonfired(&recovered),
        "teammate recovered: {}",
        describe(&recovered)
    );

    // ...and at once if its teammate dies mid-walk: a dead teammate is below
    // the threshold too, so only its being dead lets the hold go.
    let died = decide(0.0, carried(Posture::Pressured), Some(committed));
    assert!(moonfired(&died), "teammate died: {}", describe(&died));
}

/// `school`'s lockout, as an interrupt lands it.
fn lockout(school: SpellSchool) -> Aura {
    Aura {
        effect_type: AuraType::SpellSchoolLockout,
        duration: 4.0,
        magnitude: school.to_lockout_magnitude(),
        break_on_damage_threshold: -1.0,
        ability_name: "Counterspell".into(),
        ..Default::default()
    }
}

/// A Druid that cannot heal owes no reach, so there is no walk to hold for:
/// locked out of Nature, it Moonfires (Arcane) at once rather than hold the
/// global cooldown through the lockout for a heal it cannot cast. Locked out
/// of a school its heals are not in, it holds as before.
#[test]
fn a_druid_that_cannot_heal_holds_nothing_for_the_walk() {
    let nature_locked = scene(AiProfile::TeamPlan, OUT_OF_RANGE, DYING, Vec::new())
        .aura_full(DRUID, lockout(SpellSchool::Nature))
        .decide(DRUID);
    assert!(
        moonfired(&nature_locked),
        "Nature locked: {}",
        describe(&nature_locked)
    );

    let frost_locked = scene(AiProfile::TeamPlan, OUT_OF_RANGE, DYING, Vec::new())
        .aura_full(DRUID, lockout(SpellSchool::Frost))
        .decide(DRUID);
    assert!(
        held(&frost_locked),
        "Frost locked: {}",
        describe(&frost_locked)
    );
}

/// The `TeamPlan` walk feeds only the Druid's hold: the Shaman's cast
/// deferral (`escape_defer`) stays the `Legacy` walk's, so a PRESSURED
/// `TeamPlan` Shaman walked toward a dying teammate out of reach still casts
/// its Lightning Bolt. Its pair: under `Legacy`, the medic walk to the same
/// teammate behind a pillar defers the bolt.
#[test]
fn the_teamplan_walk_leaves_the_shaman_cast_deferral_alone() {
    let shaman = |profile: AiProfile, warrior: (f32, f32), obstacles: Vec<ObstacleVolume>| {
        Scene::new()
            .now(NOW)
            .ai_profile(1, profile)
            .unit(SHAMAN, 1, Shaman)
            .targeting(ENEMY)
            .unit(WARRIOR, 1, Warrior)
            .at(warrior.0, warrior.1)
            .health(DYING)
            .unit(ENEMY, 2, Mage)
            .at(0.0, 25.0)
            // Every totem down and Frost Shock spent: the bolt is the turn.
            .totems(SHAMAN, [60.0; 4])
            .cooldown(SHAMAN, AbilityType::FrostShock, 6.0)
            .healer_posture(SHAMAN, pressured())
            .obstacles(obstacles)
            .decide(SHAMAN)
    };
    let deferred = |view: &DecisionView| {
        matches!(
            view.rejected(AbilityType::LightningBolt),
            Some(RejectionReason::PreconditionUnmet { note }) if note.contains("medic walk")
        )
    };

    let team_plan = shaman(AiProfile::TeamPlan, OUT_OF_RANGE, Vec::new());
    assert!(
        matches!(
            team_plan.directive.map(|d| d.goal),
            Some(MovementGoal::Point(p)) if p.x > 0.5
        ),
        "the solve walks the Shaman toward the Warrior: {}",
        describe(&team_plan)
    );
    assert_eq!(
        team_plan.chose(),
        Some((AbilityType::LightningBolt, Some(ENEMY))),
        "TeamPlan walk, no deferral: {}",
        describe(&team_plan)
    );

    let legacy = shaman(AiProfile::Legacy, BEHIND_PILLAR, pillar());
    assert!(
        deferred(&legacy),
        "Legacy medic walk defers: {}",
        describe(&legacy)
    );
}
