//! AS-389 — the team's target focus in a played match (tier 3, staged).
//!
//! TwinPillars' west pillar stands at (-9, 0). Our Mage, west of it, targets
//! the enemy Priest standing east of it — out of the Mage's sight — while the
//! enemy Warrior stands north-west, in the Mage's reach and sight. Every unit
//! is frozen in place (no speed; the Priests with no mana), so what decides
//! the Mage's target is the focus alone: the situation holds by
//! construction, whatever the seed.
//!
//! - With the focus on, the team's focus moves to the Warrior once the
//!   Priest has been out of the Mage's sight for `FOCUS_HIDDEN_SECS`, and the
//!   Mage's target follows on the next frame.
//! - With it withheld (`team1_target_focus: false`), the Mage keeps the
//!   hidden Priest for the whole window.
//! - With the player's kill call on the Priest (`team1_kill_target`), the
//!   call wins and the Mage keeps it.

mod common;

use arenasim::headless::{FrameObservation, HeadlessMatchConfig, Stage, Unit};
use arenasim::states::play_match::class_ai::target_focus::FOCUS_HIDDEN_SECS;
use arenasim::states::play_match::AbilityType;
use bevy::prelude::*;
use common::staged::{play, Played};

const FRAME: f32 = 1.0 / 60.0;
/// The window read: long enough for the threshold and a frame, short of
/// anything else changing the scene.
const WINDOW: f32 = 4.0;

const MAGE: Unit = Unit::new(1, 0);
const HEALER: Unit = Unit::new(1, 1);
const ENEMY_PRIEST: Unit = Unit::new(2, 0);
const ENEMY_WARRIOR: Unit = Unit::new(2, 1);

fn config(seed: u64, focus: bool, kill_call: bool) -> HeadlessMatchConfig {
    HeadlessMatchConfig {
        team1: vec!["Mage".into(), "Priest".into()],
        team2: vec!["Priest".into(), "Warrior".into()],
        map: "TwinPillars".to_string(),
        ai_profile: Some("TeamPlan".into()),
        team1_target_focus: Some(focus),
        team1_kill_target: kill_call.then_some(0),
        random_seed: Some(seed),
        max_duration_secs: 8.0,
        ..Default::default()
    }
}

fn stage() -> Stage {
    let frozen = |c: &mut arenasim::states::play_match::Combatant| c.base_movement_speed = 0.0;
    Stage::new()
        .place(MAGE, -25.0, 0.0)
        .target(MAGE, ENEMY_PRIEST)
        .combatant(MAGE, frozen)
        .cooldown(MAGE, AbilityType::FrostNova, 60.0)
        .cooldown(MAGE, AbilityType::Polymorph, 60.0)
        .place(HEALER, -30.0, 6.0)
        .mana(HEALER, 0.0)
        .combatant(HEALER, frozen)
        .place(ENEMY_PRIEST, -2.0, 0.0)
        .mana(ENEMY_PRIEST, 0.0)
        .combatant(ENEMY_PRIEST, frozen)
        .place(ENEMY_WARRIOR, -20.0, 12.0)
        .target(ENEMY_WARRIOR, HEALER)
        .cooldown(ENEMY_WARRIOR, AbilityType::Charge, 60.0)
        .combatant(ENEMY_WARRIOR, frozen)
}

fn find(frame: &FrameObservation, who: Unit) -> (Entity, &arenasim::headless::ObservedCombatant) {
    frame
        .combatants
        .iter()
        .find(|(_, c)| c.team == who.team && c.slot == who.slot && !c.is_pet)
        .map(|(e, c)| (*e, c))
        .expect("the unit")
}

/// The Mage's target on each frame of the window, as the unit it names.
fn mage_targets(played: &Played) -> Vec<(f32, Option<Unit>)> {
    let start = played.landed().app;
    played
        .staged_frames()
        .take((WINDOW / FRAME) as usize)
        .map(|f| {
            let (_, mage) = find(f, MAGE);
            let named = mage.target.map(|t| {
                if t == find(f, ENEMY_PRIEST).0 {
                    ENEMY_PRIEST
                } else if t == find(f, ENEMY_WARRIOR).0 {
                    ENEMY_WARRIOR
                } else {
                    panic!("the Mage targets neither enemy")
                }
            });
            (f.sim_time - start, named)
        })
        .collect()
}

fn focus_changes(played: &Played) -> Vec<serde_json::Value> {
    played
        .staged_trace()
        .filter(|e| e["kind"] == "situation_change" && e["name"] == "target_focus")
        .cloned()
        .collect()
}

#[test]
fn a_mage_leaves_a_hidden_priest_for_the_warrior_it_sees() {
    for seed in [1, 2, 3] {
        let on = play(config(seed, true, false), stage());
        let targets = mage_targets(&on);
        let switched = targets
            .iter()
            .find(|(_, t)| *t == Some(ENEMY_WARRIOR))
            .map(|(at, _)| *at);
        println!("seed {seed}: switched at {switched:?}");
        let at = switched.unwrap_or_else(|| panic!("seed {seed}: the Mage never switched"));
        assert!(
            (FOCUS_HIDDEN_SECS..FOCUS_HIDDEN_SECS + 0.25).contains(&at),
            "seed {seed}: switched at {at:.2}s, not just past the threshold"
        );
        assert!(
            targets
                .iter()
                .filter(|(t, _)| *t > at)
                .all(|(_, w)| *w == Some(ENEMY_WARRIOR)),
            "seed {seed}: the Mage did not stay on the Warrior"
        );
        let changes = focus_changes(&on);
        assert_eq!(changes.len(), 1, "seed {seed}: {changes:?}");
        assert_eq!(changes[0]["change"], "onset");
        assert_eq!(changes[0]["team"], 1);

        let off = play(config(seed, false, false), stage());
        assert!(
            mage_targets(&off)
                .iter()
                .all(|(_, t)| *t == Some(ENEMY_PRIEST)),
            "seed {seed}: withheld, the Mage left its target"
        );
        assert!(focus_changes(&off).is_empty());

        let called = play(config(seed, true, true), stage());
        assert!(
            mage_targets(&called)
                .iter()
                .all(|(_, t)| *t == Some(ENEMY_PRIEST)),
            "seed {seed}: the kill call did not win over the focus"
        );
    }
}
