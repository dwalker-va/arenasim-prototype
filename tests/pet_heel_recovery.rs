//! AS-402 — a pet at Heel recovers and comes back to the fight.
//!
//! The defect was found in a played match: a Spider knocked below the Heel
//! threshold (25%) sat at its Hunter's side at 15% for the remaining two
//! minutes. Nothing heals a pet, and Heel had no way out. A heeled pet now
//! recovers health (`pet_ai::recover_heeled_pets`) and stays at Heel until it
//! is back to the release threshold (`pet_ai::heel_latch`), then re-engages.
//!
//! The latch is pinned case by case in `class_ai/pet_ai.rs`. This is the
//! played guard, a STAGED match (tier 3, `headless::stage`): the pet's health
//! is set on the gate frame, so no seed can stop reaching the situation. Each
//! case is a pair — the pet staged at Heel, and its control staged in the band
//! between the thresholds without the latch.

mod common;

use arenasim::headless::{FrameObservation, HeadlessMatchConfig, ObservedCombatant, Stage, Unit};
use arenasim::states::play_match::class_ai::pet_ai::{
    PET_HEEL_ENTER_HP, PET_HEEL_RECOVERY_PER_SEC, PET_HEEL_RELEASE_HP,
};
use arenasim::states::play_match::components::{Aura, AuraType};
use bevy::prelude::Entity;
use common::staged::{play, sim_time, Played};

const PET: Unit = Unit::pet_of(1, 0);
const WARRIOR: Unit = Unit::new(2, 0);

/// Below the Heel threshold, as the Spider was in the reported match.
const HEELED_HP: f32 = 0.15;

/// Between the two thresholds: a pet that heeled stays at Heel here, one that
/// did not keeps fighting.
const BETWEEN_HP: f32 = (PET_HEEL_ENTER_HP + PET_HEEL_RELEASE_HP) / 2.0;

/// How long the stun cases hold the pet.
const STUN_SECS: f32 = 6.0;

fn config(pet: &str) -> HeadlessMatchConfig {
    HeadlessMatchConfig {
        team1: vec!["Hunter".into()],
        team2: vec!["Warrior".into()],
        team1_hunter_pet_types: vec![pet.into()],
        map: "BasicArena".into(),
        random_seed: Some(7),
        max_duration_secs: 45.0,
        ..Default::default()
    }
}

/// One played match with the Hunter's `pet` staged at `hp` on the gate frame.
fn played(pet: &str, hp: f32) -> Played {
    play(config(pet), Stage::new().health(PET, hp))
}

/// The same, with the pet stunned for [`STUN_SECS`] as the stage lands.
fn played_stunned(hp: f32) -> Played {
    let stun = Aura {
        effect_type: AuraType::Stun,
        duration: STUN_SECS,
        break_on_damage_threshold: -1.0,
        ability_name: "Kidney Shot".into(),
        ..Default::default()
    };
    play(
        config("Spider"),
        Stage::new().health(PET, hp).aura(PET, stun),
    )
}

/// The Hunter's pet's entity.
fn pet_entity(frame: &FrameObservation) -> Entity {
    *frame
        .combatants
        .iter()
        .find(|(_, c)| c.is_pet && c.team == 1)
        .expect("the Hunter's pet")
        .0
}

/// The Hunter's pet's row on a frame.
fn pet(frame: &FrameObservation) -> &ObservedCombatant {
    frame
        .combatants
        .values()
        .find(|c| c.is_pet && c.team == 1)
        .expect("the Hunter's pet")
}

fn hp(frame: &FrameObservation) -> f32 {
    let s = pet(frame);
    s.current_health / s.max_health
}

/// The first staged frame on which the pet has a target — it is in the
/// fight — while alive.
fn first_engaged(played: &Played) -> Option<&FrameObservation> {
    played
        .staged_frames()
        .find(|f| pet(f).alive && pet(f).target.is_some())
}

/// A pet staged below the Heel threshold heels, recovers, and comes back to
/// the fight once it is back to the release threshold — and its Hunter, which
/// reads the same latch, sends it no command while it recovers.
fn heels_recovers_and_re_engages(pet_type: &str, command: &str) {
    let played = played(pet_type, HEELED_HP);
    let landed = played.landed().app;

    // It heeled: the frames after the stage leave it without a target.
    let heeled = played
        .staged_frames()
        .take_while(|f| pet(f).target.is_none())
        .count();
    assert!(
        heeled > 60,
        "the {pet_type} staged at {HEELED_HP} should sit at Heel for more than a second, \
         sat {heeled} frames"
    );

    let engaged = first_engaged(&played).unwrap_or_else(|| {
        panic!(
            "the {pet_type} never came back to the fight; last health {:.3}",
            hp(played.frames.last().unwrap())
        )
    });
    // It came back RECOVERED, at the release threshold — not as soon as it
    // crossed the Heel threshold.
    assert!(
        hp(engaged) >= PET_HEEL_RELEASE_HP - 0.01,
        "the {pet_type} re-engaged at {:.3} health, below the release threshold \
         {PET_HEEL_RELEASE_HP}",
        hp(engaged)
    );
    // And its health rose all the way there while it sat at Heel: nobody on
    // its team heals, so only the recovery can have raised it.
    let rise = hp(engaged) - HEELED_HP;
    assert!(
        rise > 0.3,
        "the {pet_type}'s health should have risen from {HEELED_HP} to the release \
         threshold, rose {rise:.3}"
    );

    // Its Hunter reads the same latch: while the pet recovers between the two
    // thresholds, the Hunter's `command` is refused at Heel, and none goes out
    // (a command sent to a heeled pet is dropped unexecuted).
    let offset = played.landed().app - played.landed().combat;
    let engaged_at = engaged.sim_time - offset;
    let hp_of = |e: &serde_json::Value| e["actor"]["hp_pct"].as_f64().unwrap() as f32;
    let window: Vec<&serde_json::Value> = played
        .staged_trace()
        .filter(|e| sim_time(e) < engaged_at)
        .filter(|e| e["kind"] == "pet_decision" && !e["dispatched_by"].is_null())
        .collect();
    let refused_in_band = window
        .iter()
        .filter(|e| {
            hp_of(e) >= PET_HEEL_ENTER_HP
                && e["candidates"].as_array().is_some_and(|c| {
                    c.iter()
                        .any(|c| c["ability"] == command && c["reason"] == "LowHealthHeel")
                })
        })
        .count();
    assert!(
        refused_in_band > 0,
        "the Hunter should have been refused {command} above the Heel threshold while \
         the {pet_type} recovered"
    );
    let sent: Vec<f32> = window
        .iter()
        // The frame the pet reaches the release threshold, the Hunter
        // (deciding first) may already command it: it is released that frame.
        .filter(|e| hp_of(e) < PET_HEEL_RELEASE_HP)
        .filter(|e| e["outcome"]["type"] == "action_taken")
        .map(|e| sim_time(e))
        .collect();
    assert!(
        sent.is_empty(),
        "the Hunter commanded its heeled {pet_type} at {sent:?}"
    );

    eprintln!(
        "{pet_type} re-engaged {:.2}s after the stage at {:.3} health",
        engaged.sim_time - landed,
        hp(engaged)
    );
}

/// A pet staged in the band without having heeled is in the fight at once —
/// the release threshold holds only a pet already at Heel.
fn fights_on_between_the_thresholds(pet_type: &str) {
    let played = played(pet_type, BETWEEN_HP);
    let landed = played.landed().app;
    let engaged = first_engaged(&played).expect("the pet engages");
    assert!(
        engaged.sim_time - landed < 0.5,
        "a {pet_type} at {BETWEEN_HP} that never heeled should engage at once, took {:.2}s",
        engaged.sim_time - landed
    );
    assert!(
        hp(engaged) < PET_HEEL_RELEASE_HP,
        "the control must engage inside the band, engaged at {:.3}",
        hp(engaged)
    );
}

#[test]
fn a_heeled_spider_recovers_and_re_engages() {
    heels_recovers_and_re_engages("Spider", "SpiderWeb");
}

#[test]
fn a_spider_between_the_thresholds_that_never_heeled_fights_on() {
    fights_on_between_the_thresholds("Spider");
}

/// The Bird's command (Master's Call) is refused through its own path in the
/// Hunter's dispatch, so it is played too.
#[test]
fn a_heeled_bird_recovers_and_re_engages() {
    heels_recovers_and_re_engages("Bird", "MastersCall");
}

#[test]
fn a_bird_between_the_thresholds_that_never_heeled_fights_on() {
    fights_on_between_the_thresholds("Bird");
}

/// The pet's health rise over the stun, from the stage to the frame it ends.
fn rise_while_stunned(played: &Played) -> f32 {
    let landed = played.landed().app;
    let during: Vec<&FrameObservation> = played
        .staged_frames()
        .take_while(|f| f.sim_time < landed + STUN_SECS - 0.1)
        .collect();
    assert!(
        during
            .iter()
            .all(|f| pet(f).aura_types.contains(&AuraType::Stun)),
        "the pet should be stunned throughout"
    );
    hp(during.last().unwrap()) - hp(during[0])
}

/// A pet knocked below the threshold while held in crowd control goes to Heel
/// all the same, and recovers through the stun rather than after it.
#[test]
fn a_pet_stunned_below_the_threshold_recovers_through_the_stun() {
    let rise = rise_while_stunned(&played_stunned(HEELED_HP));
    assert!(
        rise > 0.08,
        "a stunned pet at {HEELED_HP} should recover through a {STUN_SECS}s stun, rose {rise:.3}"
    );
}

/// Control: a pet stunned in the band, never at Heel, does not recover.
#[test]
fn a_pet_stunned_between_the_thresholds_does_not_recover() {
    let rise = rise_while_stunned(&played_stunned(BETWEEN_HP));
    assert!(
        rise <= 0.0,
        "a stunned pet at {BETWEEN_HP} that never heeled should not recover, rose {rise:.3}"
    );
}

/// A pet that dies at Heel stays dead: the recovery heals only a living pet.
/// Its pair is `a_heeled_spider_recovers_and_re_engages`, the same latch on a
/// living pet.
#[test]
fn a_pet_that_dies_at_heel_stays_dead() {
    let played = play(
        config("Spider"),
        Stage::new().combatant(PET, |c| {
            c.pet_heeled = true;
            c.current_health = 0.0;
            c.is_dead = true;
        }),
    );
    let raised: Vec<f32> = played
        .staged_frames()
        .filter(|f| pet(f).current_health > 0.0)
        .map(|f| f.sim_time)
        .collect();
    assert!(
        raised.is_empty(),
        "a dead pet at Heel was raised to health at {:?}",
        raised.first()
    );
    assert!(
        played.staged_frames().count() > 60,
        "the match should play on past the pet's death"
    );
}

/// Mortal Wound's cut, as Mortal Strike and Aimed Shot land it: 0.65 of the
/// healing received.
const MORTAL_WOUND: f32 = 0.65;

/// How long a recovery is measured: ten ticks, with half a tick to spare.
const WINDOW_SECS: f32 = 10.5;

/// The Hunter's pet staged at Heel with the enemy Warrior stunned past the
/// window, so nothing touches the pet's health but its recovery; with a
/// Mortal Wound of `wound` on the pet when given.
fn played_recovering(wound: Option<f32>) -> Played {
    let held = Aura {
        effect_type: AuraType::Stun,
        duration: WINDOW_SECS + 5.0,
        break_on_damage_threshold: -1.0,
        ability_name: "Kidney Shot".into(),
        ..Default::default()
    };
    let mut stage = Stage::new().health(PET, HEELED_HP).aura(WARRIOR, held);
    if let Some(magnitude) = wound {
        stage = stage.aura(
            PET,
            Aura {
                effect_type: AuraType::HealingReduction,
                duration: WINDOW_SECS + 5.0,
                magnitude,
                break_on_damage_threshold: -1.0,
                ability_name: "Mortal Strike".into(),
                ..Default::default()
            },
        );
    }
    play(config("Spider"), stage)
}

/// The staged frames inside the window.
fn in_window(played: &Played) -> Vec<&FrameObservation> {
    let landed = played.landed().app;
    let frames: Vec<&FrameObservation> = played
        .staged_frames()
        .take_while(|f| f.sim_time < landed + WINDOW_SECS)
        .collect();
    assert!(
        frames.last().unwrap().sim_time > landed + WINDOW_SECS - 0.1,
        "the match should play through the window"
    );
    assert!(
        frames
            .iter()
            .all(|f| pet(f).alive && pet(f).target.is_none()),
        "the pet should sit at Heel, alive, throughout the window"
    );
    frames
}

/// The pet's health rise over the window.
fn recovery_over_window(played: &Played) -> f32 {
    let frames = in_window(played);
    hp(frames.last().unwrap()) - hp(frames[0])
}

/// The window's sim times at which a Mortal Wounds tell naming the pet was
/// spawned, asserting each frame spawned at most one.
fn tells_over_window(played: &Played) -> Vec<f32> {
    let frames = in_window(played);
    let pet = pet_entity(frames[0]);
    let count = |f: &FrameObservation| f.healing_refused.get(&pet).copied().unwrap_or(0);
    let mut tells = Vec::new();
    for pair in frames.windows(2) {
        let new = count(pair[1]) - count(pair[0]);
        assert!(
            new <= 1,
            "{new} tells named the pet on one frame, at {:.2}",
            pair[1].sim_time
        );
        if new == 1 {
            tells.push(pair[1].sim_time);
        }
    }
    tells
}

/// Under a Mortal Wound the recovery runs at the wound's cut of its rate —
/// healing reduction cuts it, as it cuts every heal.
#[test]
fn a_mortal_wound_cuts_a_heeled_pets_recovery() {
    let wounded = recovery_over_window(&played_recovering(Some(MORTAL_WOUND)));
    let clean = recovery_over_window(&played_recovering(None));
    let ratio = wounded / clean;
    assert!(
        (ratio - MORTAL_WOUND).abs() < 0.02,
        "a pet under a {MORTAL_WOUND} Mortal Wound should recover at {MORTAL_WOUND} of the \
         rate, recovered {wounded:.3} against {clean:.3} ({ratio:.3})"
    );
}

/// Control: with nothing on it, the pet recovers at the full rate.
#[test]
fn a_clean_heeled_pet_recovers_at_the_full_rate() {
    let clean = recovery_over_window(&played_recovering(None));
    let ticks = WINDOW_SECS.floor();
    let full = PET_HEEL_RECOVERY_PER_SEC * ticks;
    assert!(
        (clean - full).abs() < 0.005,
        "a clean pet should recover {full:.3} in {ticks} ticks, recovered {clean:.3}"
    );
}

/// The recovery lands in ticks, so a wounded pet shows the Mortal Wounds tell
/// at most once a second — not once a frame.
#[test]
fn a_wounded_heeled_pet_tells_at_most_once_a_second() {
    let tells = tells_over_window(&played_recovering(Some(MORTAL_WOUND)));
    assert!(
        tells.len() >= WINDOW_SECS.floor() as usize - 1,
        "a wounded pet recovering at Heel should show the tell on its ticks, showed it at \
         {tells:?}"
    );
    for pair in tells.windows(2) {
        assert!(
            pair[1] - pair[0] > 0.95,
            "two tells {:.3}s apart, at {:.2} and {:.2}",
            pair[1] - pair[0],
            pair[0],
            pair[1]
        );
    }
}

/// Control: a pet with no healing reduction on it shows no tell.
#[test]
fn a_clean_heeled_pet_shows_no_tell() {
    let tells = tells_over_window(&played_recovering(None));
    assert!(
        tells.is_empty(),
        "a pet with no Mortal Wound showed the tell at {tells:?}"
    );
}

/// Seconds from the stage to the pet's first recovery tick, the pet staged at
/// Heel's threshold with `timer` seconds already on its tick clock and with
/// the Heel latch `heeled` or not; the Warrior is held, so nothing else
/// touches the pet's health.
fn first_tick_after_stage(heeled: bool, timer: f32) -> f32 {
    let held = Aura {
        effect_type: AuraType::Stun,
        duration: 5.0,
        break_on_damage_threshold: -1.0,
        ability_name: "Kidney Shot".into(),
        ..Default::default()
    };
    let played = play(
        config("Spider"),
        Stage::new()
            .health(PET, HEELED_HP)
            .combatant(PET, move |c| {
                c.pet_heeled = heeled;
                c.pet_heel_recovery_timer = timer;
            })
            .aura(WARRIOR, held),
    );
    let landed = played.landed().app;
    let start = hp(played.staged_frames().next().unwrap());
    let ticked = played
        .staged_frames()
        .find(|f| hp(f) > start + 0.001)
        .expect("the pet recovers")
        .sim_time;
    ticked - landed
}

/// A pet that goes to Heel starts its tick clock afresh: time left on it from
/// an earlier Heel does not bring the first tick forward.
#[test]
fn a_new_heel_starts_its_tick_clock_afresh() {
    let first = first_tick_after_stage(false, 0.9);
    assert!(
        first > 0.95,
        "a pet that has just heeled should recover a full tick later, recovered after {first:.2}s"
    );
}

/// Control: a pet already at Heel keeps its clock, so the same time on it
/// brings the tick forward.
#[test]
fn a_heel_in_progress_keeps_its_tick_clock() {
    let first = first_tick_after_stage(true, 0.9);
    assert!(
        first < 0.2,
        "a heeled pet with 0.9s on its clock should tick at once, ticked after {first:.2}s"
    );
}
