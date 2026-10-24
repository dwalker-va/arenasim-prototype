//! The Peel play's situation-conditional probe (tier 4 of the AI testing
//! pyramid, `docs/design/proposals/2026-10-11-ai-architecture.md` §3.2 and
//! §5 step 5).
//!
//! It plays a SEED BANK of 2v2 matches under `TeamPlan` twice: with the Peel
//! play, and with it withheld from both teams (`team{1,2}_withheld_plays`).
//! In each it finds every `ally_in_danger` window with the classifier the AI
//! itself uses — read off the decision trace's `situation_change` onsets — and
//! measures two things per window:
//!
//! - **answered**: within one global cooldown of the onset, a teammate of the
//!   unit in danger chose one of its class's peel tools
//!   (`class_ai::peel::peel_tools`) on one of the enemies targeting that unit
//!   at the onset — a targeted cast on it, or a Frost Nova / Psychic Scream
//!   with it inside the area;
//! - **survival**: the seconds from the onset until the unit in danger died,
//!   capped at [`SURVIVAL_HORIZON`] (a unit alive at the cap, or at the
//!   match's end, counts the whole horizon).
//!
//! Only the windows the play is ENABLED for are counted: those where a living
//! teammate of the unit in danger has a peel tool and is not switched off for
//! that unit's class (`class_ai::peel::PEEL_GATES`). A window only a gated
//! peeler could answer is not one the play is asked to answer, so it is left
//! out of both arms alike — which windows count is a property of the comp,
//! never of the arm — and only an enabled peeler's cast answers one.
//!
//! Both are asserted against the SAME bank with the play withheld, in units
//! of the bank's own noise (a z), never against an absolute number of the
//! metric (§7: a rate threshold fitted to a number is a seed floor by another
//! name). The answered rate must be significantly HIGHER; the survival must
//! not be significantly LOWER — the bank cannot resolve the survival gain the
//! play buys (the findings note, `docs/design/balance/2026-10-12-as274-peel-play.md`),
//! so it is held to doing no harm. The windows themselves differ between the
//! arms — the play changes the matches — which is the point of a rate over
//! windows: drift changes which windows exist, not the assertion. A thin bank
//! fails as THIN, with the window count, not as a broken behaviour.
//!
//! `#[ignore]`d: it plays 384 full matches (about a minute in release). Run it with
//!
//! ```text
//! cargo test --release --test peel_play_probe -- --ignored --nocapture
//! ```

mod common;

use std::collections::BTreeMap;

use arenasim::headless::{FrameObservation, HeadlessMatchConfig};
use arenasim::states::match_config::CharacterClass;
use arenasim::states::play_match::ability_config::load_ability_definitions;
use arenasim::states::play_match::class_ai::peel::{gated, peel_tools};
use arenasim::states::play_match::AbilityType;
use common::staged::{play_seeded, Played};

/// One global cooldown: the window in which a peel counts as the answer.
const ANSWER_WITHIN: f32 = 1.5;
/// How long a unit in danger is followed for its survival.
const SURVIVAL_HORIZON: f32 = 10.0;
/// Fewer windows than this in either arm and the bank is too thin to say.
const MIN_WINDOWS: usize = 300;
/// How far above the withheld bank, in its own standard errors, the play's
/// answered rate must sit.
const ANSWERED_Z: f64 = 3.0;
/// How far below the withheld bank, in standard errors, the ally's mean
/// survival may fall before the play is shortening its life.
const HARM_Z: f64 = -2.0;

const COMPS: &[(&[&str], &[&str])] = &[
    (&["Mage", "Priest"], &["Warrior", "Druid"]),
    (&["Warlock", "Shaman"], &["Rogue", "Priest"]),
    (&["Hunter", "Paladin"], &["Warrior", "Mage"]),
    (&["Rogue", "Druid"], &["Hunter", "Priest"]),
    (&["Warrior", "Shaman"], &["Rogue", "Warlock"]),
    (&["Paladin", "Mage"], &["Warrior", "Hunter"]),
];
const MAPS: &[&str] = &["PillaredArena", "BasicArena"];
const SEEDS: std::ops::Range<u64> = 1..17;

fn config(team1: &[&str], team2: &[&str], map: &str, seed: u64, peel: bool) -> HeadlessMatchConfig {
    let withheld = if peel {
        Vec::new()
    } else {
        vec!["Peel".to_string()]
    };
    HeadlessMatchConfig {
        team1: team1.iter().map(|s| s.to_string()).collect(),
        team2: team2.iter().map(|s| s.to_string()).collect(),
        map: map.to_string(),
        random_seed: Some(seed),
        ai_profile: Some("TeamPlan".to_string()),
        team1_withheld_plays: withheld.clone(),
        team2_withheld_plays: withheld,
        ..Default::default()
    }
}

/// One `ally_in_danger` window and what came of it.
#[derive(Debug)]
struct Window {
    answered: bool,
    survival: f32,
}

/// The frame observation taken after the update that recorded `event`: the
/// trace's frame counter starts one update behind the observations (checked
/// in [`windows`] against the classifier's own dying test).
fn frame_of(event: &serde_json::Value) -> usize {
    event["frame"].as_u64().expect("frame") as usize + 1
}

fn class_of(name: &str) -> CharacterClass {
    CharacterClass::all()
        .iter()
        .copied()
        .find(|c| format!("{c:?}") == name)
        .unwrap_or_else(|| panic!("class {name}"))
}

fn ability_of(name: &str) -> Option<AbilityType> {
    serde_json::from_value(serde_json::Value::String(name.to_string())).ok()
}

/// Every `ally_in_danger` window of one played match.
fn windows(played: &Played) -> Vec<Window> {
    let defs = load_ability_definitions().expect("abilities.ron loads");
    let frames = &played.frames;
    let entity_by_index: BTreeMap<u64, bevy::prelude::Entity> = frames[0]
        .combatants
        .keys()
        .map(|e| (e.index() as u64, *e))
        .collect();
    let onsets = played.trace.iter().filter(|e| {
        e["kind"] == "situation_change" && e["name"] == "ally_in_danger" && e["change"] == "onset"
    });
    let mut out = Vec::new();
    for onset in onsets {
        let team = onset["team"].as_u64().unwrap() as u8;
        let subject_index = onset["subject"].as_u64().unwrap();
        let subject = entity_by_index[&subject_index];
        let t0 = onset["sim_time"].as_f64().unwrap() as f32;
        let f0 = frame_of(onset).min(frames.len() - 1);
        let at_onset: &FrameObservation = &frames[f0];
        // The classifier's dying test, on the observation of the onset's
        // update or the one before it (a heal can land after the decision in
        // the same update): the trace's frames and the observations line up.
        // An update in which the unit's health moved can have crossed the line
        // and back inside itself — a tick before the decision, a heal after
        // it — which no end-of-update observation shows; it is accepted too.
        let dying_at = |f: usize| {
            let s = &frames[f].combatants[&subject];
            s.current_health < s.max_health * 0.5
        };
        let moved_in = |f: usize| {
            f > 0
                && frames[f].combatants[&subject].current_health
                    != frames[f - 1].combatants[&subject].current_health
        };
        assert!(
            dying_at(f0) || (f0 > 0 && dying_at(f0 - 1)) || moved_in(f0),
            "frame {f0}: the unit in danger is not dying in the observation — the trace \
             frame and the frame observations are out of step"
        );
        // The peelers the play is enabled for: living teammates, not the unit
        // in danger, with a peel tool and no gate for its class.
        let subject_class = at_onset.combatants[&subject].class;
        let enabled =
            |class: CharacterClass| !peel_tools(class).is_empty() && !gated(class, subject_class);
        if !at_onset.combatants.iter().any(|(e, c)| {
            c.team == team && c.alive && !c.is_pet && *e != subject && enabled(c.class)
        }) {
            continue;
        }
        let attackers: Vec<u64> = at_onset
            .combatants
            .iter()
            .filter(|(_, c)| c.team != team && c.alive && c.target == Some(subject))
            .map(|(e, _)| e.index() as u64)
            .collect();
        let answered = played.trace.iter().any(|e| {
            if e["kind"] != "ability_decision" || e["outcome"]["type"] != "action_taken" {
                return false;
            }
            let t = e["sim_time"].as_f64().unwrap() as f32;
            let actor = &e["actor"];
            if t < t0
                || t > t0 + ANSWER_WITHIN
                || actor["team"].as_u64() != Some(team as u64)
                || actor["entity_id"].as_u64() == Some(subject_index)
            {
                return false;
            }
            let class = class_of(actor["class"].as_str().unwrap());
            let Some(ability) = e["outcome"]["ability"].as_str().and_then(ability_of) else {
                return false;
            };
            if !enabled(class) || !peel_tools(class).contains(&ability) {
                return false;
            }
            match e["outcome"]["target_id"].as_u64() {
                Some(target) => attackers.contains(&target),
                // A self-centred area: an attacker inside it.
                None => {
                    let f = frame_of(e).min(frames.len() - 1);
                    let p = &actor["position"];
                    let caster = bevy::prelude::Vec3::new(
                        p[0].as_f64().unwrap() as f32,
                        p[1].as_f64().unwrap() as f32,
                        p[2].as_f64().unwrap() as f32,
                    );
                    let reach = defs.get(&ability).map_or(0.0, |d| d.range);
                    attackers.iter().any(|a| {
                        frames[f].combatants[&entity_by_index[a]]
                            .position
                            .distance(caster)
                            <= reach
                    })
                }
            }
        });
        let death = frames[f0..]
            .iter()
            .find(|f| !f.combatants[&subject].alive)
            .map(|f| f.sim_time - at_onset.sim_time);
        let survival = death.unwrap_or(SURVIVAL_HORIZON).min(SURVIVAL_HORIZON);
        out.push(Window { answered, survival });
    }
    out
}

/// The bank's windows under one arm.
fn bank(peel: bool) -> Vec<Window> {
    let mut all = Vec::new();
    for (team1, team2) in COMPS {
        for map in MAPS {
            for seed in SEEDS {
                let played = play_seeded(config(team1, team2, map, seed, peel));
                all.extend(windows(&played));
            }
        }
    }
    all
}

struct Summary {
    windows: usize,
    answered: usize,
    mean_survival: f64,
    /// The sample variance of the per-window survival.
    var_survival: f64,
    survived_horizon: usize,
}

fn summarise(label: &str, w: &[Window]) -> Summary {
    let n = w.len().max(1) as f64;
    let mean = w.iter().map(|w| w.survival as f64).sum::<f64>() / n;
    let var = w
        .iter()
        .map(|w| (w.survival as f64 - mean).powi(2))
        .sum::<f64>()
        / (n - 1.0).max(1.0);
    let s = Summary {
        windows: w.len(),
        answered: w.iter().filter(|w| w.answered).count(),
        mean_survival: mean,
        var_survival: var,
        survived_horizon: w.iter().filter(|w| w.survival >= SURVIVAL_HORIZON).count(),
    };
    let answered: Vec<f32> = w
        .iter()
        .filter(|w| w.answered)
        .map(|w| w.survival)
        .collect();
    eprintln!(
        "{label}: {} windows, {} answered within one GCD ({:.1}%), mean survival {:.2}s \
         (cap {SURVIVAL_HORIZON}s; {:.2}s in the answered windows), {} reached the cap ({:.1}%)",
        s.windows,
        s.answered,
        100.0 * s.answered as f64 / n,
        s.mean_survival,
        answered.iter().sum::<f32>() / answered.len().max(1) as f32,
        s.survived_horizon,
        100.0 * s.survived_horizon as f64 / n,
    );
    s
}

/// The answered rate's two-proportion z, `with` over `without` (pooled).
fn answered_z(with: &Summary, without: &Summary) -> f64 {
    let (k1, n1) = (with.answered as f64, with.windows as f64);
    let (k2, n2) = (without.answered as f64, without.windows as f64);
    let p = (k1 + k2) / (n1 + n2);
    (k1 / n1 - k2 / n2) / (p * (1.0 - p) * (1.0 / n1 + 1.0 / n2)).sqrt()
}

/// The mean survival's Welch z, `with` over `without`.
fn survival_z(with: &Summary, without: &Summary) -> f64 {
    let se = (with.var_survival / with.windows as f64
        + without.var_survival / without.windows as f64)
        .sqrt();
    (with.mean_survival - without.mean_survival) / se
}

/// The play must answer MORE danger windows with crowd control on the
/// attacker than the same bank without it — significantly, by the bank's own
/// spread (z of at least [`ANSWERED_Z`]) — and must not shorten the ally's
/// survival: its mean survival may not fall significantly below the
/// withheld bank's (z above [`HARM_Z`]). Both are relative to the paired bank,
/// in units of its own noise, never a fitted number.
#[test]
#[ignore = "plays 384 full matches; run with --release -- --ignored"]
fn peel_answers_more_danger_windows_without_shortening_the_allys_life() {
    let with = summarise("peel", &bank(true));
    let without = summarise("withheld", &bank(false));
    assert!(
        with.windows >= MIN_WINDOWS && without.windows >= MIN_WINDOWS,
        "the bank is thin: {} and {} ally-in-danger windows (need {MIN_WINDOWS} in each) — \
         add comps or seeds, the behaviour is not what failed",
        with.windows,
        without.windows
    );
    let (az, sz) = (answered_z(&with, &without), survival_z(&with, &without));
    eprintln!(
        "answered z {az:.2}; survival z {sz:.2} (with {:.2}s, without {:.2}s)",
        with.mean_survival, without.mean_survival
    );
    assert!(
        az >= ANSWERED_Z,
        "the play does not answer significantly more danger windows with crowd control on \
         the attacker than the same bank without it: {}/{} vs {}/{} (z {az:.2})",
        with.answered,
        with.windows,
        without.answered,
        without.windows
    );
    assert!(
        sz > HARM_Z,
        "the ally in danger dies significantly sooner with the play than without it: \
         {:.2}s vs {:.2}s (z {sz:.2})",
        with.mean_survival,
        without.mean_survival
    );
}
