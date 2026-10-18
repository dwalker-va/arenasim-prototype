//! AS-209 — the Priest's Psychic Scream dip aborts for a dying teammate,
//! whether the Priest reaches it or not.
//!
//! The dip's abort (`scream_dip_should_abort`) reads health only: a teammate
//! at or below `urgency_hp_threshold` ends the dip the tick the posture
//! machine sees it, reached or not, so reach needs no rule of its own here —
//! the HP rule already covers every dying teammate the solve's `C_DYING`
//! would. A dip that outlasts its 6s budget is a dip the Priest spent in
//! crowd control: no posture runs while it is incapacitated, and the dip
//! aborts on its first free tick.
//!
//! Each scene is a pair with its one condition removed, under both profiles.

use arenasim::states::match_config::CharacterClass::{Priest, Warrior};
use arenasim::states::play_match::ai_profile::AiProfile;
use arenasim::states::play_match::class_ai::scene::{DecisionView, Scene};
use arenasim::states::play_match::components::{HealerPosture, MovementGoal, Posture};
use arenasim::states::play_match::decision_trace::MovementTrigger;

const PRIEST: &str = "priest";
const WARRIOR: &str = "warrior";
const ENEMY_PRIEST: &str = "enemy priest";

/// At `urgency_hp_threshold` (0.5) or below: dying.
const DYING: f32 = 0.45;
/// Above it: hurt, not dying.
const HURT: f32 = 0.55;

const NOW: f32 = 30.0;

/// 45yd east of the Priest: beyond the 40yd heal range.
const OUT_OF_REACH: (f32, f32) = (45.0, 0.0);
/// 10yd east: in heal range and sight.
const IN_REACH: (f32, f32) = (10.0, 0.0);

/// A Priest at the origin on team 1 under `profile`, two seconds into a dip
/// walking at the enemy Priest 20yd west (which attacks nobody), its teammate
/// Warrior at `warrior` and `warrior_hp`.
fn decide(profile: AiProfile, warrior: (f32, f32), warrior_hp: f32) -> DecisionView {
    let scene = Scene::new()
        .now(NOW)
        .ai_profile(1, profile)
        .unit(PRIEST, 1, Priest)
        .unit(WARRIOR, 1, Warrior)
        .at(warrior.0, warrior.1)
        .health(warrior_hp)
        .unit(ENEMY_PRIEST, 2, Priest)
        .at(-20.0, 0.0);
    let target = scene.entity(ENEMY_PRIEST);
    scene
        .healer_posture(
            PRIEST,
            HealerPosture {
                posture: Posture::Dip,
                dip_target: Some(target),
                dip_until: NOW + 4.0,
                ..HealerPosture::new(NOW - 2.0)
            },
        )
        .decide(PRIEST)
}

fn aborted(view: &DecisionView) -> bool {
    view.movement
        .iter()
        .any(|m| m.trigger == MovementTrigger::DipAbort)
}

/// Still dipping: no abort, and the walk at the enemy Priest goes on.
fn dipping(view: &DecisionView) -> bool {
    !aborted(view)
        && matches!(
            view.directive.map(|d| d.goal),
            Some(MovementGoal::Entity(_))
        )
}

fn describe(view: &DecisionView) -> String {
    format!(
        "movement {:?}; directive {:?}",
        view.movement,
        view.directive.map(|d| d.goal)
    )
}

/// A dying teammate out of the Priest's reach ends the dip; the same teammate
/// hurt but not dying leaves it running.
#[test]
fn a_dying_teammate_out_of_reach_aborts_the_dip() {
    for profile in [AiProfile::TeamPlan, AiProfile::Legacy] {
        let dying = decide(profile, OUT_OF_REACH, DYING);
        assert!(aborted(&dying), "{profile:?}, dying: {}", describe(&dying));

        let hurt = decide(profile, OUT_OF_REACH, HURT);
        assert!(dipping(&hurt), "{profile:?}, hurt: {}", describe(&hurt));
    }
}

/// The abort reads health, not reach: a dying teammate in reach ends the dip
/// too, and one in reach that is not dying does not.
#[test]
fn the_abort_reads_health_not_reach() {
    for profile in [AiProfile::TeamPlan, AiProfile::Legacy] {
        let dying = decide(profile, IN_REACH, DYING);
        assert!(aborted(&dying), "{profile:?}, dying: {}", describe(&dying));

        let hurt = decide(profile, IN_REACH, HURT);
        assert!(dipping(&hurt), "{profile:?}, hurt: {}", describe(&hurt));
    }
}
