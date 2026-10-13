//! The Burst play in a STAGED match (tier 3, `headless::stage`): a Hunter's
//! kill target is low and the enemy Priest is stunned for 2s — a kill window
//! that closes before an Aimed Shot (2.5s and its flight) could land. Under
//! the play the Hunter bursts with Arcane Shot, which lands inside the window;
//! with the play withheld, the lock-window rule starts the Aimed Shot, which
//! lands after it.
//!
//! The scene tests (`tests/burst_scenes.rs`) pin the one-tick decision; this
//! pins that the decision reaches the match: the shot goes through the real
//! schedule and HITS before the lock ends.

mod common;

use arenasim::headless::{HeadlessMatchConfig, Stage, Unit};
use arenasim::states::play_match::{Aura, AuraType};
use common::staged::{entity_id, of_class, play, sim_time, Played};

const HUNTER: Unit = Unit::new(1, 0);
const PRIEST: Unit = Unit::new(1, 1);
const ENEMY_WARRIOR: Unit = Unit::new(2, 0);
const ENEMY_PRIEST: Unit = Unit::new(2, 1);

/// The stun on the enemy Priest: the kill window's length.
const LOCK: f32 = 2.0;

fn config(seed: u64, burst: bool) -> HeadlessMatchConfig {
    let withheld = if burst {
        Vec::new()
    } else {
        vec!["Burst".to_string()]
    };
    HeadlessMatchConfig {
        team1: vec!["Hunter".into(), "Priest".into()],
        team2: vec!["Warrior".into(), "Priest".into()],
        map: "BasicArena".into(),
        ai_profile: Some("TeamPlan".into()),
        team1_withheld_plays: withheld,
        random_seed: Some(seed),
        max_duration_secs: 15.0,
        ..Default::default()
    }
}

/// The enemy Warrior at 20%, on the Hunter's Priest 30yd from the Hunter; the
/// enemy Priest stunned for [`LOCK`], 43yd off; the Hunter targeting the
/// Warrior, nothing within 20yd of it.
fn stage() -> Stage {
    Stage::new()
        .place(HUNTER, -10.0, 0.0)
        .place(ENEMY_WARRIOR, 20.0, 0.0)
        .health(ENEMY_WARRIOR, 0.2)
        .place(PRIEST, 17.0, -3.0)
        .target(ENEMY_WARRIOR, PRIEST)
        .target(HUNTER, ENEMY_WARRIOR)
        .place(ENEMY_PRIEST, 25.0, 25.0)
        .aura(
            ENEMY_PRIEST,
            Aura {
                effect_type: AuraType::Stun,
                duration: LOCK,
                break_on_damage_threshold: -1.0,
                ability_name: "Kidney Shot".into(),
                ..Default::default()
            },
        )
}

/// When (seconds after the stage) the Hunter's `shot` first hit the enemy
/// Warrior, from the combat log (the app's clock).
fn hit_after_stage(played: &Played, shot: &str) -> Option<f32> {
    let landed = played.landed().app;
    played.log.lines().find_map(|line| {
        if !(line.contains("[DMG] Team 1 Hunter")
            && line.contains(&format!("'s {shot} hits Team 2 Warrior")))
        {
            return None;
        }
        let stamp: f32 = line
            .trim_start_matches('[')
            .split('s')
            .next()?
            .trim()
            .parse()
            .ok()?;
        (stamp >= landed).then_some(stamp - landed)
    })
}

#[test]
fn the_hunter_bursts_with_a_shot_that_lands_before_the_lock_ends() {
    for seed in [1, 2] {
        let played = play(config(seed, true), stage());
        let start = played.landed().combat;
        let warrior = entity_id(&played, 2, 0);
        let window = played
            .staged_trace()
            .find(|e| e["kind"] == "situation_change" && e["name"] == "kill_window")
            .unwrap_or_else(|| panic!("seed {seed}: no kill window opened"));
        assert_eq!(window["subject"].as_u64(), Some(warrior), "seed {seed}");
        let until = window["until"]
            .as_f64()
            .expect("a lock's window has an end") as f32;
        assert!(
            (until - start - LOCK).abs() < 0.1,
            "seed {seed}: until {until}"
        );

        let burst = of_class(played.staged_trace(), "Hunter")
            .filter(|e| e["kind"] == "ability_decision" && e["actor"]["team"] == 1)
            .find(|e| e["situation"]["play_step"] == "burst")
            .unwrap_or_else(|| panic!("seed {seed}: the Hunter never bursts"));
        assert!(sim_time(burst) - start <= 0.5, "seed {seed}: burst late");
        assert_eq!(burst["outcome"]["ability"], "ArcaneShot", "seed {seed}");
        assert_eq!(burst["outcome"]["target_id"].as_u64(), Some(warrior));
        // The Aimed Shot that would land after the lock was refused for it.
        assert!(
            burst["candidates"].as_array().unwrap().iter().any(|c| {
                c["ability"] == "AimedShot" && c["reason"]["OutlastsWindow"].is_object()
            }),
            "seed {seed}: {burst}"
        );
        // ...and no Aimed Shot was begun on the Warrior inside the window.
        assert!(
            of_class(played.staged_trace(), "Hunter").all(|e| {
                let t = sim_time(e);
                !(e["outcome"]["ability"] == "AimedShot" && t < until)
            }),
            "seed {seed}: an Aimed Shot begun inside the window"
        );
        let hit = hit_after_stage(&played, "Arcane Shot")
            .unwrap_or_else(|| panic!("seed {seed}: the Arcane Shot never hit"));
        assert!(
            hit <= LOCK,
            "seed {seed}: it hit {hit:.2}s in, after the lock"
        );

        // Control: the play withheld. The lock-window rule starts Aimed Shot,
        // which lands after the lock, and no shot hits inside it.
        let played = play(config(seed, false), stage());
        assert!(
            of_class(played.staged_trace(), "Hunter")
                .all(|e| e["situation"]["play_step"].is_null()),
            "seed {seed}: a burst with the play withheld"
        );
        let first = of_class(played.staged_trace(), "Hunter")
            .find(|e| e["kind"] == "ability_decision" && e["outcome"]["type"] == "action_taken")
            .unwrap_or_else(|| panic!("seed {seed}: the Hunter never acts"));
        assert_eq!(first["outcome"]["ability"], "AimedShot", "seed {seed}");
        assert!(
            hit_after_stage(&played, "Arcane Shot").is_none_or(|t| t > LOCK),
            "seed {seed}: without the play an Arcane Shot hit inside the lock"
        );
        let aimed = hit_after_stage(&played, "Aimed Shot")
            .unwrap_or_else(|| panic!("seed {seed}: the Aimed Shot never hit"));
        assert!(
            aimed > LOCK,
            "seed {seed}: the Aimed Shot hit at {aimed:.2}s"
        );
    }
}
