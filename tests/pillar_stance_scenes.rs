//! AS-387 (AS-385 card B) — the team pillar stance and the Pillar play, one
//! classification or one tick at a time (tier 1).
//!
//! A `TeamPlan` team takes a pillar TOGETHER (`class_ai::pillar_stance`): the
//! stance enters for `AvoidCc` when a crowd control a pillar defends against
//! is ready for a teammate, the fight stands at a pillar, the pillar's ring has
//! a TEAM arc (ground from which the healer sees every teammate, out of that
//! crowd control's reach), and no kill is on. Under it the healer hugs the
//! pillar on the team arc, a kiting ranged DPS bends toward the stance's side,
//! and a melee is left to its own chain. Each scene is a pair — the situation,
//! and its control with one condition removed — so none can pass vacuously.

use arenasim::states::match_config::CharacterClass::{
    self, Druid, Hunter, Mage, Paladin, Priest, Rogue, Warlock, Warrior,
};
use arenasim::states::play_match::ai_profile::AiProfile;
use arenasim::states::play_match::class_ai::pillar_mode::{
    HUG_ENTER, HUG_LEAVE, HUG_RELEASE, MODE_DWELL,
};
use arenasim::states::play_match::class_ai::pillar_stance::{
    StanceReason, StanceWindow, PILLAR_EXCEPTIONS, STALL_COMPS,
};
use arenasim::states::play_match::class_ai::scene::{DecisionView, Scene};
use arenasim::states::play_match::components::MovementGoal;
use arenasim::states::play_match::decision_trace::{MovementTrigger, PlayTag};
use arenasim::states::play_match::map_geometry::{has_line_of_sight, ObstacleVolume, EYE_HEIGHT};
use arenasim::states::play_match::situation::{ClassSet, Play, PlaySet};
use arenasim::states::play_match::AuraType;
use bevy::prelude::*;

const HEALER: &str = "healer";
const MATE: &str = "warrior";
const MAGE: &str = "mage";
const CASTER: &str = "caster";
const BRUISER: &str = "bruiser";

/// A Nagrand-sized pillar at the origin: footprint disc radius 6, ring at 8.
fn pillar() -> Vec<ObstacleVolume> {
    vec![ObstacleVolume::Cylinder {
        center_xz: Vec2::ZERO,
        radius: 6.0,
        base_y: 0.0,
        height: 10.0,
    }]
}

fn eye(p: Vec3) -> Vec3 {
    Vec3::new(p.x, EYE_HEIGHT, p.z)
}

/// Our healer west of the pillar; our Warrior fighting an enemy Warrior north
/// of it, `mate_skin` yards from its skin; an enemy `caster` east of it
/// targeting the Warrior. Both teams run `TeamPlan`.
fn scene(healer: CharacterClass, caster: CharacterClass, mate_skin: f32) -> Scene {
    let z = 6.0 + mate_skin;
    Scene::new()
        .obstacles(pillar())
        .ai_profile(1, AiProfile::TeamPlan)
        .ai_profile(2, AiProfile::TeamPlan)
        .unit(HEALER, 1, healer)
        .at(-14.0, 0.0)
        .unit(MATE, 1, Warrior)
        .at(0.0, z)
        .targeting(BRUISER)
        .unit(BRUISER, 2, Warrior)
        .at(1.5, z)
        .targeting(MATE)
        .unit(CASTER, 2, caster)
        .at(18.0, 2.0)
        .targeting(MATE)
}

/// Team 1's stance after one classification of `s`.
fn stance(s: &mut Scene) -> Option<StanceWindow> {
    s.reclassify().teams[0].pillar_stance
}

fn entered(view: &DecisionView) -> bool {
    view.movement
        .iter()
        .any(|m| m.trigger == MovementTrigger::PillarModeEnter)
}

fn play_of(view: &DecisionView) -> Option<PlayTag> {
    view.situation.as_ref().and_then(|s| s.play)
}

// ---------------------------------------------------------------------------
// The keys
// ---------------------------------------------------------------------------

/// A Warlock with Fear ready, the fight at the pillar, a team arc: the team
/// takes the pillar, for `AvoidCc`, judged from its healer. Each control
/// removes one condition: the Warlock silenced (no crowd control ready), the
/// play withheld, `Legacy`, a map with no pillar, the fight beyond the
/// stance's entry edge.
#[test]
fn a_ready_fear_at_a_pillar_fight_takes_the_pillar() {
    let mut s = scene(Priest, Warlock, 6.0);
    let healer = s.entity(HEALER);
    let w = stance(&mut s).expect("the team takes the pillar");
    assert_eq!(w.reason, StanceReason::AvoidCc);
    assert_eq!(w.pillar, 0);
    assert_eq!(w.healer, healer);
    // The side sees the Warrior and is hidden from the Warlock.
    let side = Vec2::from_angle(w.side) * 8.0;
    let side = Vec3::new(side.x, 1.0, side.y);
    assert!(has_line_of_sight(
        &pillar(),
        eye(side),
        eye(Vec3::new(0.0, 1.0, 12.0))
    ));
    assert!(!has_line_of_sight(
        &pillar(),
        eye(side),
        eye(Vec3::new(18.0, 1.0, 2.0))
    ));

    let mut silenced = scene(Priest, Warlock, 6.0).aura(CASTER, AuraType::Silence, 10.0);
    assert_eq!(stance(&mut silenced), None, "silenced Warlock");
    let mut withheld = scene(Priest, Warlock, 6.0).withhold(1, PlaySet::of(&[Play::Pillar]));
    assert_eq!(stance(&mut withheld), None, "Pillar withheld");
    let mut legacy = scene(Priest, Warlock, 6.0).ai_profile(1, AiProfile::Legacy);
    assert_eq!(stance(&mut legacy), None, "Legacy");
    let mut open_map = scene(Priest, Warlock, 6.0).obstacles(Vec::new());
    assert_eq!(stance(&mut open_map), None, "no pillar");
    let mut far = scene(Priest, Warlock, HUG_ENTER + 0.5);
    assert_eq!(stance(&mut far), None, "the fight beyond the entry edge");
    let mut near = scene(Priest, Warlock, HUG_ENTER - 0.5);
    assert!(
        stance(&mut near).is_some(),
        "the fight inside the entry edge"
    );
}

/// A pillar is a defence against a crowd control that needs sight and is cast
/// from range, and against nothing else: a Rogue walks round it to land its
/// stuns. The Rogue alone gives no stance; a Mage (Polymorph) does.
#[test]
fn a_rogue_alone_takes_no_pillar() {
    assert_eq!(stance(&mut scene(Priest, Rogue, 6.0)), None);
    assert!(stance(&mut scene(Priest, Mage, 6.0)).is_some());
}

/// A kill window is the Burst play's: the stance is not taken while one is
/// open. The enemy has no healer, so its Warrior at 20% is a kill.
#[test]
fn a_kill_window_takes_no_pillar() {
    let mut calm = scene(Priest, Warlock, 6.0);
    let classified = calm.reclassify().teams[0].clone();
    assert!(classified.kill_window.is_none());
    assert!(classified.pillar_stance.is_some(), "no kill: the stance");
    let mut kill = scene(Priest, Warlock, 6.0);
    kill.set_health(BRUISER, 0.2);
    let classified = kill.reclassify().teams[0].clone();
    assert!(classified.kill_window.is_some(), "the scene opens a kill");
    assert_eq!(classified.pillar_stance, None, "a kill is on");
}

/// A wall between the Warrior and the whole ring: no ground at the pillar sees
/// it, so the team cannot take the pillar together.
#[test]
fn an_empty_ring_arc_takes_no_pillar() {
    let mut walled = pillar();
    walled.push(ObstacleVolume::Aabb {
        min: Vec3::new(-30.0, 0.0, 9.5),
        max: Vec3::new(30.0, 6.0, 10.5),
    });
    assert_eq!(
        stance(&mut scene(Priest, Warlock, 6.0).obstacles(walled)),
        None
    );
}

// ---------------------------------------------------------------------------
// The team arc
// ---------------------------------------------------------------------------

/// A 3v3 round the pillar: our Warrior at its north skin and our Mage either
/// at its south skin or beside the Warrior. With the two on opposite sides no
/// ring spot sees both — the anchor's own arc exists, but the TEAM arc does
/// not — so the team takes no pillar; with the Mage beside the Warrior it
/// does. A stance judged on the anchor's arc alone takes the pillar in both.
fn three_v_three(mage_at: (f32, f32)) -> Scene {
    Scene::new()
        .obstacles(pillar())
        .ai_profile(1, AiProfile::TeamPlan)
        .ai_profile(2, AiProfile::TeamPlan)
        .unit(HEALER, 1, Priest)
        .at(-14.0, 0.0)
        .unit(MATE, 1, Warrior)
        .at(0.0, 7.5)
        .health(0.6)
        .targeting(BRUISER)
        .unit(MAGE, 1, Mage)
        .at(mage_at.0, mage_at.1)
        .targeting(BRUISER)
        .unit(BRUISER, 2, Warrior)
        .at(1.5, 7.5)
        .targeting(MATE)
        .unit(CASTER, 2, Warlock)
        .at(30.0, 2.0)
        .targeting(MATE)
}

#[test]
fn the_team_arc_needs_every_teammate() {
    let mut split = three_v_three((0.0, -7.5));
    assert_eq!(stance(&mut split), None, "no spot sees both");
    let mut together = three_v_three((-3.0, 9.0));
    let w = stance(&mut together).expect("both north: the team arc exists");
    // The side sees both teammates.
    let side = Vec2::from_angle(w.side) * 8.0;
    for mate in [Vec3::new(0.0, 1.0, 7.5), Vec3::new(-3.0, 1.0, 9.0)] {
        assert!(has_line_of_sight(
            &pillar(),
            eye(Vec3::new(side.x, 1.0, side.y)),
            eye(mate)
        ));
    }
}

// ---------------------------------------------------------------------------
// Hysteresis
// ---------------------------------------------------------------------------

/// The fight's distance is a Schmitt pair: a stance enters at `HUG_ENTER`
/// from the skin, holds out to `HUG_LEAVE`, and lets go beyond it (once its
/// dwell has run). Classified frame after frame, the Warrior walking away.
#[test]
fn the_fight_distance_is_a_schmitt_pair() {
    let between = 6.0 + (HUG_ENTER + HUG_LEAVE) / 2.0;
    let mut s = scene(Priest, Warlock, 6.0).now(30.0);
    assert!(stance(&mut s).is_some());
    s.set_now(30.0 + MODE_DWELL + 0.1);
    s.set_position(MATE, 0.0, between);
    s.set_position(BRUISER, 1.5, between);
    assert!(stance(&mut s).is_some(), "let go inside the band");
    s.set_now(30.0 + MODE_DWELL + 0.2);
    s.set_position(MATE, 0.0, 6.0 + HUG_LEAVE + 1.0);
    s.set_position(BRUISER, 1.5, 6.0 + HUG_LEAVE + 1.0);
    assert_eq!(stance(&mut s), None, "held beyond the band");
    // Its pair: entering from cold at the same in-band distance does not.
    let mut cold = scene(Priest, Warlock, between - 6.0);
    assert_eq!(stance(&mut cold), None, "entered inside the band from cold");
}

/// The crowd-control key releases only after `HUG_RELEASE` false, and the
/// stance is held through its dwell; once it ends it is not re-entered until
/// a dwell has passed.
#[test]
fn the_threat_releases_late_and_the_stance_dwells() {
    let mut s = scene(Priest, Warlock, 6.0).now(30.0);
    assert!(stance(&mut s).is_some());
    // The Warlock's mana gone: no crowd control ready.
    s.combatant_mut(CASTER).current_mana = 0.0;
    s.set_now(30.0 + MODE_DWELL + 0.1);
    assert!(stance(&mut s).is_some(), "the threat key just went false");
    s.set_now(30.0 + MODE_DWELL + 0.1 + HUG_RELEASE - 0.2);
    assert!(stance(&mut s).is_some(), "released before HUG_RELEASE");
    s.set_now(30.0 + MODE_DWELL + 0.1 + HUG_RELEASE + 0.1);
    assert_eq!(stance(&mut s), None, "held past HUG_RELEASE");
    // Mana back: the keys hold again, but the stance just ended.
    let ended = 30.0 + MODE_DWELL + 0.1 + HUG_RELEASE + 0.1;
    s.combatant_mut(CASTER).current_mana = 1000.0;
    s.set_now(ended + MODE_DWELL / 2.0);
    assert_eq!(stance(&mut s), None, "re-entered inside the dwell");
    s.set_now(ended + MODE_DWELL + 0.1);
    assert!(stance(&mut s).is_some(), "not re-entered after the dwell");
}

/// Feasibility ends a stance at once, inside its dwell: a kill window opening
/// (the Burst play's), or the team arc lost.
#[test]
fn a_kill_or_a_lost_arc_ends_the_stance_at_once() {
    let mut s = scene(Priest, Warlock, 6.0).now(30.0);
    assert!(stance(&mut s).is_some());
    s.set_now(30.0 + MODE_DWELL / 4.0);
    assert!(stance(&mut s).is_some(), "the control frame");
    s.set_health(BRUISER, 0.2);
    assert_eq!(stance(&mut s), None, "a kill opened inside the dwell");

    let mut arc = three_v_three((-3.0, 9.0)).now(30.0);
    assert!(stance(&mut arc).is_some());
    arc.set_now(30.0 + MODE_DWELL / 4.0);
    arc.set_position(MAGE, 0.0, -7.5);
    assert_eq!(stance(&mut arc), None, "the team arc lost inside the dwell");
}

// ---------------------------------------------------------------------------
// The roles
// ---------------------------------------------------------------------------

/// Design D3: the healer hugs ONLY under its team's stance. In the AS-373
/// lone-hug situation — the Warrior at the pillar, the Warlock's Fear ready,
/// the ring sighted — a healer whose team holds no stance (the Pillar play
/// withheld) does not hug; under the stance it does, and its goal stands on
/// the ring in its Warrior's sight. A healer the play's exceptions name does
/// not hug either, though its team holds the stance: the hug is the play.
#[test]
fn the_healer_hugs_only_under_the_stance() {
    for healer in [Priest, Druid] {
        let view = scene(healer, Warlock, 6.0).decide(HEALER);
        assert!(entered(&view), "{healer:?}: {:?}", view.movement);
        assert_eq!(play_of(&view), Some(PlayTag::Pillar), "{healer:?}");
        let at = view
            .directive
            .and_then(|d| match d.goal {
                MovementGoal::Point(p) => Some(p),
                _ => None,
            })
            .expect("a point walk");
        let r = Vec2::new(at.x, at.z).length();
        assert!((r - 8.0).abs() < 0.6, "{healer:?}: goal {at} is {r}yd out");
        assert!(has_line_of_sight(
            &pillar(),
            eye(at),
            eye(Vec3::new(0.0, 1.0, 12.0))
        ));

        let alone = scene(healer, Warlock, 6.0)
            .withhold(1, PlaySet::of(&[Play::Pillar]))
            .decide(HEALER);
        assert!(!entered(&alone), "{healer:?} hugged alone");
        let mut excepted =
            scene(healer, Warlock, 6.0).exceptions(Play::Pillar, ClassSet::of(&[healer]));
        assert!(stance(&mut excepted).is_some(), "the team still takes it");
        let view = excepted.decide(HEALER);
        assert!(!entered(&view), "{healer:?}: an excepted healer hugged");
    }
}

/// The Paladin — the one healer `HUG_EXCEPTIONS` names — has no executor for
/// the healer's half, so its team takes no stance (a stance is a team
/// position judged from a healer that plays it).
#[test]
fn a_paladin_team_takes_no_pillar() {
    assert_eq!(stance(&mut scene(Paladin, Warlock, 6.0)), None);
    assert!(stance(&mut scene(Priest, Warlock, 6.0)).is_some());
}

/// A 3v3 with a Hunter kiting an enemy Rogue on the pillar's west side.
fn hunter_scene(play: bool) -> Scene {
    let s = Scene::new()
        .obstacles(pillar())
        .ai_profile(1, AiProfile::TeamPlan)
        .ai_profile(2, AiProfile::TeamPlan)
        .unit(HEALER, 1, Priest)
        .at(-14.0, 0.0)
        .unit(MATE, 1, Warrior)
        .at(0.0, 12.0)
        .targeting(BRUISER)
        .unit("hunter", 1, Hunter)
        .at(-16.0, 8.0)
        .targeting(BRUISER)
        .unit(BRUISER, 2, Warrior)
        .at(1.5, 12.0)
        .targeting(MATE)
        .unit("chaser", 2, Rogue)
        .at(-14.0, 5.0)
        .targeting("hunter")
        .unit(CASTER, 2, Warlock)
        .at(30.0, 2.0)
        .targeting(MATE);
    let mut s = if play {
        s
    } else {
        s.withhold(1, PlaySet::of(&[Play::Pillar]))
    };
    s.combatant_mut("chaser").stealthed = false;
    s
}

/// Under the stance the Hunter's KITE scorer carries the `stance_pull` term,
/// and its play is Pillar; with the play withheld the term is absent. Here
/// the pull turns the kite: with it the Hunter flees a compass step nearer
/// the stance's side (its scored direction points more toward the side's
/// ring point) than without it.
#[test]
fn a_kiting_hunter_bends_toward_the_stance_side() {
    let mut s = hunter_scene(true);
    let side = stance(&mut s).expect("the scene takes the pillar").side;
    let view = s.decide("hunter");
    assert_eq!(play_of(&view), Some(PlayTag::Pillar));
    assert!(
        view.scorer_term("stance_pull").is_some(),
        "no stance pull: {:?}",
        view.movement
    );
    let off = hunter_scene(false).decide("hunter");
    assert_eq!(
        off.scorer_term("stance_pull"),
        None,
        "withheld, still pulled"
    );

    let toward = (Vec2::from_angle(side) * 8.0 - Vec2::new(-16.0, 8.0)).normalize();
    let dir = |v: &DecisionView| {
        let d = v
            .movement
            .iter()
            .rev()
            .find_map(|m| m.chosen_direction)
            .expect("a kite direction");
        Vec2::new(d[0], d[1]).dot(toward)
    };
    assert!(
        dir(&view) > dir(&off) + 0.1,
        "the pull did not bend the kite toward the side: {} against {}",
        dir(&view),
        dir(&off)
    );
}

/// A melee is not called to the Pillar play — its half of the stance is the
/// sight yield (AS-386), read from the situation — so the stance never takes
/// it off its chain. The situation it decides under still carries the stance.
#[test]
fn a_melee_is_not_called_to_the_pillar() {
    let view = scene(Priest, Warlock, 6.0).decide(MATE);
    let situation = view.situation.expect("a situation");
    assert!(
        situation.pillar_stance.is_some(),
        "the team holds the stance"
    );
    assert_ne!(situation.play, Some(PlayTag::Pillar));
}

// ---------------------------------------------------------------------------
// The tables
// ---------------------------------------------------------------------------

/// The Pillar play ships no exceptions, and `Stall` (design D4) ships no
/// comp: an entry added to either fails here until it is named.
#[test]
fn the_pillar_and_stall_tables_ship_empty() {
    assert!(PILLAR_EXCEPTIONS.is_empty());
    assert!(STALL_COMPS.is_empty());
}

/// `Stall` is never classified while its table is empty: a Druid comp, the
/// stall's best case (design §2.4), with no crowd control ready, takes no
/// stance.
#[test]
fn no_stall_is_taken() {
    let mut s = scene(Druid, Rogue, 6.0);
    assert_eq!(stance(&mut s), None);
}
