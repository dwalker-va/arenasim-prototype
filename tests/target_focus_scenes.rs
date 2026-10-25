//! AS-389 — the team's target focus, one classification at a time (tier 1).
//!
//! A ranged DPS blind to its target for `FOCUS_HIDDEN_SECS`, with another
//! enemy in its reach and sight, moves its team's focus to that enemy
//! (`class_ai::target_focus`, `TeamSituation::target_focus`); its ranged DPS
//! take the focus in target acquisition (`target_focus::retarget`, read here
//! through `Scene::acquire`). Every condition is a pair: the situation, and
//! its control with the one condition removed.
//!
//! The ground: one pillar at the origin. Our Mage stands west of it, its
//! target — the enemy Priest — east of it, out of the Mage's sight; the enemy
//! Warrior stands north-west, in the Mage's reach and sight.

use arenasim::states::match_config::CharacterClass::{
    Hunter, Mage, Priest, Rogue, Warlock, Warrior,
};
use arenasim::states::play_match::ai_profile::AiProfile;
use arenasim::states::play_match::class_ai::scene::Scene;
use arenasim::states::play_match::class_ai::target_focus::{
    Focus, FOCUS_DWELL, FOCUS_HIDDEN_SECS, FOCUS_REACH,
};
use arenasim::states::play_match::map_geometry::ObstacleVolume;
use arenasim::states::play_match::AuraType;
use bevy::prelude::*;

const MAGE: &str = "mage";
const HEALER: &str = "healer";
const TARGET: &str = "enemy_priest";
const EXPOSED: &str = "enemy_warrior";

const T0: f32 = 30.0;
/// A sliver of a second either side of a threshold.
const EPS: f32 = 0.05;

fn pillar() -> ObstacleVolume {
    ObstacleVolume::Cylinder {
        center_xz: Vec2::ZERO,
        radius: 3.0,
        base_y: 0.0,
        height: 5.0,
    }
}

fn scene() -> Scene {
    Scene::new()
        .ai_profile(1, AiProfile::TeamPlan)
        .ai_profile(2, AiProfile::TeamPlan)
        .obstacles(vec![pillar()])
        .now(T0)
        .unit(MAGE, 1, Mage)
        .at(-20.0, 0.0)
        .targeting(TARGET)
        .unit(HEALER, 1, Priest)
        .at(-25.0, 6.0)
        .unit(TARGET, 2, Priest)
        .at(5.0, 0.0)
        .unit(EXPOSED, 2, Warrior)
        .at(-12.0, 10.0)
}

/// Team 1's focus after classifying at each of `times` in turn.
fn focus_after(s: &mut Scene, times: &[f32]) -> Option<Focus> {
    let mut focus = None;
    for t in times {
        s.set_now(*t);
        focus = s.reclassify().teams[0].target_focus;
    }
    focus
}

/// Classify from `T0` until just past the hidden threshold.
fn past_threshold(s: &mut Scene) -> Option<Focus> {
    focus_after(s, &[T0, T0 + FOCUS_HIDDEN_SECS + EPS])
}

// ---------------------------------------------------------------------------
// The onset
// ---------------------------------------------------------------------------

/// A target out of sight for the threshold, an enemy in reach and sight: the
/// focus moves to it. Control: the same enemies with the target in sight.
#[test]
fn a_hidden_target_moves_the_focus_to_an_enemy_in_sight() {
    let mut s = scene();
    let focus = past_threshold(&mut s).expect("the focus moves");
    assert_eq!(focus.subject, s.entity(EXPOSED));
    assert_eq!(focus.from, s.entity(TARGET));
    assert!((focus.since - (T0 + FOCUS_HIDDEN_SECS + EPS)).abs() < 1e-4);

    let mut sighted = scene().place(TARGET, -5.0, -15.0);
    assert_eq!(past_threshold(&mut sighted), None);
}

/// The hidden clock: just short of the threshold the focus holds; just past
/// it, it moves. A target that steps back into sight resets the clock.
#[test]
fn the_focus_waits_for_the_hidden_threshold() {
    let mut s = scene();
    assert_eq!(
        focus_after(&mut s, &[T0, T0 + FOCUS_HIDDEN_SECS - EPS]),
        None
    );
    assert!(focus_after(&mut s, &[T0 + FOCUS_HIDDEN_SECS + EPS]).is_some());

    // In sight for one frame in the middle: the clock starts again.
    let mut s = scene();
    focus_after(&mut s, &[T0, T0 + 1.0]);
    s.set_position(TARGET, -5.0, -15.0);
    focus_after(&mut s, &[T0 + 1.1]);
    s.set_position(TARGET, 5.0, 0.0);
    assert_eq!(
        focus_after(&mut s, &[T0 + 1.2, T0 + FOCUS_HIDDEN_SECS + EPS]),
        None,
        "1.2s after the reset is short of the threshold"
    );
    assert!(focus_after(&mut s, &[T0 + 1.2 + FOCUS_HIDDEN_SECS + EPS]).is_some());
}

/// No alternative, no move: the only other enemy is out of sight, or out of
/// reach, or immune. The control is the scene.
#[test]
fn no_enemy_in_reach_and_sight_holds_the_target() {
    let mut behind = scene().place(EXPOSED, 6.0, 2.0);
    assert_eq!(past_threshold(&mut behind), None, "behind the pillar");
    let mut far = scene().place(EXPOSED, -20.0, FOCUS_REACH + 2.0);
    assert_eq!(past_threshold(&mut far), None, "out of reach");
    let mut immune = scene().aura(EXPOSED, AuraType::DamageImmunity, 8.0);
    assert_eq!(past_threshold(&mut immune), None, "immune");
    let mut control = scene().place(EXPOSED, -20.0, FOCUS_REACH - 2.0);
    assert!(past_threshold(&mut control).is_some(), "just in reach");
}

/// The nearest of two enemies in sight is the focus.
#[test]
fn the_nearest_enemy_in_sight_is_the_focus() {
    let mut s = scene().unit("enemy_warlock", 2, Warlock).at(-20.0, -8.0);
    let focus = past_threshold(&mut s).expect("the focus moves");
    assert_eq!(focus.subject, s.entity("enemy_warlock"));
}

/// The switch: `Legacy` and `TeamPlan` with the focus withheld classify
/// none; the switch is per team. A melee DPS blind to its target moves
/// nothing.
#[test]
fn only_a_teamplan_ranged_dps_moves_the_focus() {
    let mut legacy = scene().ai_profile(1, AiProfile::Legacy);
    assert_eq!(past_threshold(&mut legacy), None, "Legacy");
    let mut off = scene().without_target_focus(1);
    assert_eq!(past_threshold(&mut off), None, "withheld");
    let mut other = scene().without_target_focus(2);
    assert!(
        past_threshold(&mut other).is_some(),
        "team 2's switch is its own"
    );

    let mut melee = Scene::new()
        .ai_profile(1, AiProfile::TeamPlan)
        .obstacles(vec![pillar()])
        .now(T0)
        .unit("warrior", 1, Warrior)
        .at(-20.0, 0.0)
        .targeting(TARGET)
        .unit(HEALER, 1, Priest)
        .at(-25.0, 6.0)
        .unit(TARGET, 2, Priest)
        .at(5.0, 0.0)
        .unit(EXPOSED, 2, Warrior)
        .at(-12.0, 10.0);
    assert_eq!(past_threshold(&mut melee), None, "a melee DPS");
}

/// A DPS on the kill window's subject does not leave it: the hidden target
/// is a low Warlock its healer cannot heal (stunned). Control: the Warlock
/// at full health opens no kill window, and the focus moves.
#[test]
fn a_kill_window_target_is_not_left() {
    let build = |hp: f32| {
        Scene::new()
            .ai_profile(1, AiProfile::TeamPlan)
            .obstacles(vec![pillar()])
            .now(T0)
            .unit(MAGE, 1, Mage)
            .at(-20.0, 0.0)
            .targeting("enemy_warlock")
            .unit(HEALER, 1, Priest)
            .at(-25.0, 6.0)
            .unit("enemy_warlock", 2, Warlock)
            .at(5.0, 0.0)
            .health(hp)
            .unit(TARGET, 2, Priest)
            .at(-5.0, -15.0)
            .aura(TARGET, AuraType::Stun, 6.0)
            .unit(EXPOSED, 2, Warrior)
            .at(-12.0, 10.0)
    };
    let mut low = build(0.2);
    assert!(low.reclassify().teams[0].kill_window.is_some());
    assert_eq!(past_threshold(&mut low), None);
    let mut full = build(1.0);
    assert!(past_threshold(&mut full).is_some());
}

// ---------------------------------------------------------------------------
// The dwell and the return
// ---------------------------------------------------------------------------

/// Once the focus is on the Warrior and the Mage has taken it, the Warrior
/// steps behind the pillar and the Priest out of it. Returning takes the same
/// rule as leaving: the Warrior out of sight for the threshold, AND the
/// focus's dwell run out.
fn after_onset() -> (Scene, f32) {
    let mut s = scene();
    let onset = T0 + FOCUS_HIDDEN_SECS + EPS;
    past_threshold(&mut s).expect("the focus moves");
    s.set_target(MAGE, EXPOSED);
    s.set_position(EXPOSED, 6.0, 2.0);
    s.set_position(TARGET, -5.0, -15.0);
    (s, onset)
}

/// The dwell: the Warrior hidden from the onset on, the threshold passes
/// before the dwell does, and the focus holds until the dwell is out.
#[test]
fn a_focus_holds_its_dwell() {
    const { assert!(FOCUS_DWELL > FOCUS_HIDDEN_SECS + 0.2) };
    let (mut s, onset) = after_onset();
    let warrior = s.entity(EXPOSED);
    let held = focus_after(
        &mut s,
        &[
            onset + 0.02,
            onset + FOCUS_HIDDEN_SECS + 0.1,
            onset + FOCUS_DWELL - EPS,
        ],
    );
    assert_eq!(held.map(|f| f.subject), Some(warrior), "inside the dwell");
    let back = focus_after(&mut s, &[onset + FOCUS_DWELL + EPS]).expect("a focus");
    assert_eq!(back.subject, s.entity(TARGET), "the dwell out, it returns");
    assert_eq!(back.from, warrior);
}

/// The return's hysteresis: past the dwell, the old target back in sight
/// does not pull the focus back until the focus itself has been out of sight
/// for the threshold.
#[test]
fn the_return_waits_for_the_focus_to_hide_for_the_threshold() {
    let (mut s, onset) = after_onset();
    // The Warrior in sight through the dwell; it hides only after.
    s.set_position(EXPOSED, -12.0, 10.0);
    let hide = onset + FOCUS_DWELL + 0.5;
    focus_after(&mut s, &[onset + 0.02, hide - 0.1]);
    s.set_position(EXPOSED, 6.0, 2.0);
    let held = focus_after(&mut s, &[hide, hide + FOCUS_HIDDEN_SECS - EPS]);
    assert_eq!(held.map(|f| f.subject), Some(s.entity(EXPOSED)));
    let back = focus_after(&mut s, &[hide + FOCUS_HIDDEN_SECS + EPS]);
    assert_eq!(back.map(|f| f.subject), Some(s.entity(TARGET)));
}

/// The focus ends when its subject dies (or cannot be taken any more).
#[test]
fn a_dead_focus_ends() {
    let (mut s, onset) = after_onset();
    s.set_position(EXPOSED, -12.0, 10.0);
    focus_after(&mut s, &[onset + 0.1]);
    s.set_health(EXPOSED, 0.0);
    s.combatant_mut(EXPOSED).current_health = 0.0;
    assert_eq!(focus_after(&mut s, &[onset + 0.2]), None);
}

/// The focus ends when its subject goes where the team cannot take it:
/// into stealth, or immune to damage. The control: the same frame with the
/// subject still visible and open, and the focus holds. (The held focus is
/// judged by acquisition's own validity, not by alive-and-not-immune alone.)
#[test]
fn a_focus_ends_when_its_subject_stealths_or_turns_immune() {
    let build = || {
        let mut s = Scene::new()
            .ai_profile(1, AiProfile::TeamPlan)
            .ai_profile(2, AiProfile::TeamPlan)
            .obstacles(vec![pillar()])
            .now(T0)
            .unit(MAGE, 1, Mage)
            .at(-20.0, 0.0)
            .targeting(TARGET)
            .unit(HEALER, 1, Priest)
            .at(-25.0, 6.0)
            .unit(TARGET, 2, Priest)
            .at(5.0, 0.0)
            .unit("enemy_rogue", 2, Rogue)
            .at(-12.0, 10.0);
        // Out of stealth to begin with: it opened on someone.
        s.combatant_mut("enemy_rogue").stealthed = false;
        let focus = past_threshold(&mut s).expect("the focus moves");
        assert_eq!(focus.subject, s.entity("enemy_rogue"));
        s.set_target(MAGE, "enemy_rogue");
        s
    };
    let after = T0 + FOCUS_HIDDEN_SECS + EPS + 0.1;

    let mut control = build();
    assert!(
        focus_after(&mut control, &[after]).is_some(),
        "the control holds"
    );

    let mut stealthed = build();
    stealthed.combatant_mut("enemy_rogue").stealthed = true;
    assert_eq!(focus_after(&mut stealthed, &[after]), None, "stealthed");

    let mut immune = build().aura("enemy_rogue", AuraType::DamageImmunity, 8.0);
    assert_eq!(focus_after(&mut immune, &[after]), None, "immune");
}

// ---------------------------------------------------------------------------
// Taking the focus (target acquisition)
// ---------------------------------------------------------------------------

/// A second ranged DPS on the team's kill window subject keeps it, though
/// the focus moved and stands in its reach and sight. The Mage's hidden
/// target (a stunned enemy Priest) moves the focus to the enemy Warrior; the
/// Hunter is on an enemy Warlock at 20%, which the stunned Priest cannot
/// heal. Control: the Warlock at full health opens no kill window, and the
/// Hunter converges on the focus.
#[test]
fn a_second_dps_on_the_kill_window_subject_keeps_it() {
    let build = |hp: f32| {
        Scene::new()
            .ai_profile(1, AiProfile::TeamPlan)
            .ai_profile(2, AiProfile::TeamPlan)
            .obstacles(vec![pillar()])
            .now(T0)
            .unit(MAGE, 1, Mage)
            .at(-20.0, 0.0)
            .targeting(TARGET)
            .unit("hunter", 1, Hunter)
            .at(-25.0, -5.0)
            .targeting("enemy_warlock")
            .unit(HEALER, 1, Priest)
            .at(-25.0, 6.0)
            .unit(TARGET, 2, Priest)
            .at(5.0, 0.0)
            .aura(TARGET, AuraType::Stun, 6.0)
            .unit("enemy_warlock", 2, Warlock)
            .at(-30.0, -15.0)
            .health(hp)
            .unit(EXPOSED, 2, Warrior)
            .at(-12.0, 10.0)
    };
    let mut low = build(0.2);
    let focus = past_threshold(&mut low).expect("the Mage moves the focus");
    assert_eq!(focus.subject, low.entity(EXPOSED));
    assert_eq!(
        low.reclassify().teams[0].kill_window.map(|w| w.subject),
        Some(low.entity("enemy_warlock"))
    );
    assert_eq!(
        low.acquire("hunter"),
        None,
        "the kill window's subject is kept"
    );
    assert_eq!(low.acquire(MAGE), Some(EXPOSED), "the Mage still takes it");

    let mut full = build(1.0);
    past_threshold(&mut full).expect("the focus moves");
    assert!(full.reclassify().teams[0].kill_window.is_none());
    assert_eq!(
        full.acquire("hunter"),
        Some(EXPOSED),
        "no window: it converges"
    );
}

/// The Mage takes the team's focus; without one (no classification yet, or
/// the target in sight) it keeps its target.
#[test]
fn a_ranged_dps_takes_the_focus() {
    let mut s = scene();
    assert_eq!(s.acquire(MAGE), None, "no focus yet");
    past_threshold(&mut s);
    assert_eq!(s.acquire(MAGE), Some(EXPOSED));
    let mut sighted = scene().place(TARGET, -5.0, -15.0);
    past_threshold(&mut sighted);
    assert_eq!(sighted.acquire(MAGE), None);
}

/// A melee DPS does not take the focus. Control: a ranged DPS where it
/// stands does.
#[test]
fn a_melee_dps_does_not_take_the_focus() {
    let build = |class| {
        scene()
            .unit("second", 1, class)
            .at(-18.0, 4.0)
            .targeting(TARGET)
    };
    let mut melee = build(Warrior);
    past_threshold(&mut melee);
    assert_eq!(melee.acquire("second"), None);
    let mut ranged = build(Hunter);
    past_threshold(&mut ranged);
    assert_eq!(ranged.acquire("second"), Some(EXPOSED));
}

/// A second ranged DPS takes the focus when it reaches it (in reach and
/// sight), or when it cannot see its own target either. Control: the focus
/// beyond its reach with its own target in sight — it keeps its target.
#[test]
fn a_second_ranged_dps_takes_the_focus_it_reaches_or_when_blind() {
    let build = || {
        scene()
            .unit("hunter", 1, Hunter)
            .at(-50.0, 0.0)
            .targeting(TARGET)
    };
    // Beyond reach of the Warrior (about 39yd), its own target hidden.
    let mut blind = build();
    past_threshold(&mut blind);
    assert_eq!(blind.acquire("hunter"), Some(EXPOSED));
    // The same, its own target in sight: it keeps it.
    let mut sighted = build();
    past_threshold(&mut sighted);
    sighted.set_position(TARGET, -30.0, -20.0);
    assert_eq!(sighted.acquire("hunter"), None);
    // In reach and sight of the focus, its own target in sight: it converges.
    let mut near = build();
    past_threshold(&mut near);
    near.set_position(TARGET, -30.0, -20.0);
    near.set_position("hunter", -40.0, 5.0);
    assert_eq!(near.acquire("hunter"), Some(EXPOSED));
}

/// A focus the DPS cannot take — immune — is not taken.
#[test]
fn an_immune_focus_is_not_taken() {
    let mut s = scene();
    past_threshold(&mut s);
    assert_eq!(s.acquire(MAGE), Some(EXPOSED), "the control");
    let immune = s.aura(EXPOSED, AuraType::DamageImmunity, 8.0);
    assert_eq!(immune.acquire(MAGE), None);
}
