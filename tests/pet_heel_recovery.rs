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
use arenasim::states::play_match::class_ai::pet_ai::{PET_HEEL_ENTER_HP, PET_HEEL_RELEASE_HP};
use arenasim::states::play_match::components::{Aura, AuraType};
use common::staged::{play, sim_time, Played};

const PET: Unit = Unit::pet_of(1, 0);

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
