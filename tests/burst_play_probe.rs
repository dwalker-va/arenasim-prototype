//! The Burst play's situation-conditional probe (tier 4 of the AI testing
//! pyramid, `docs/design/proposals/2026-10-11-ai-architecture.md` §3.2 and
//! §5 step 6).
//!
//! It plays a SEED BANK of 2v2 matches under `TeamPlan` twice: with the Burst
//! play, and with it excepted everywhere on both teams (`team{1,2}_exceptions`
//! naming `plays::every_exception_entry`: the play graduated and cannot be
//! withheld).
//! In each it finds every `kill_window` with the classifier the AI itself
//! uses — read off the decision trace's `situation_change` events (onset,
//! extended, end). A burst LANDS inside a window when one of the class's
//! burst tools (`class_ai::burst::burst_tools`), chosen on the window's
//! subject while the window was open, arrives — its cast time, its flight
//! from where the caster stood and, for damage over time alone, its first
//! tick — by the window's `until` (or, with no `until` known, by its end).
//! Per window it measures:
//!
//! - **response**: of the decisions the window's DPS made inside it that
//!   acted, how many chose a burst tool on the subject — the tier-4 response
//!   rate — and how many of those landed inside it;
//! - **answered**: whether any burst landed inside it;
//! - **kill secured**: whether the subject died while it was open.
//!
//! All three are asserted against the SAME bank with the play withheld, in
//! units of the bank's own noise (a two-proportion z), never against an
//! absolute number of the metric (§7: a rate threshold fitted to a number is
//! a seed floor by another name). The response rate must be significantly
//! HIGHER; the landed responses, the answered windows and the kills secured
//! must not be significantly LOWER. The windows differ between the arms — the play
//! changes the matches — which is the point of a rate over windows: drift
//! changes which windows exist, not the assertion. A thin bank fails as THIN,
//! with the window count, not as a broken behaviour. The response z pools
//! decisions across windows, which are clustered, so it is optimistic.
//!
//! `#[ignore]`d: it plays 2,304 full matches (several minutes in release). Run it with
//!
//! ```text
//! cargo test --release --test burst_play_probe -- --ignored --nocapture
//! ```

mod common;

use std::collections::BTreeMap;

use arenasim::headless::HeadlessMatchConfig;
use arenasim::states::match_config::CharacterClass;
use arenasim::states::play_match::ability_config::{load_ability_definitions, AbilityDefinitions};
use arenasim::states::play_match::class_ai::burst::burst_tools;
use arenasim::states::play_match::class_ai::plays::every_exception_entry;
use arenasim::states::play_match::situation::PlayId;
use arenasim::states::play_match::{AbilityType, AuraType};
use common::staged::{play_seeded, Played};

/// One global cooldown: a burst chosen this soon after the onset answers it
/// PROMPTLY.
const ONE_GCD: f32 = 1.5;
/// Fewer windows than this in either arm and the bank is too thin to say.
const MIN_WINDOWS: usize = 300;
/// How far above the withheld bank, in its own standard errors, the share of
/// kill-window decisions the play spends on the subject must sit.
const RESPONSE_Z: f64 = 3.0;
/// How far below the withheld bank, in standard errors, the answered windows
/// and the kills secured may fall before the play is doing harm.
const HARM_Z: f64 = -2.0;

const COMPS: &[(&[&str], &[&str])] = &[
    (&["Warrior", "Priest"], &["Mage", "Druid"]),
    (&["Mage", "Shaman"], &["Rogue", "Priest"]),
    (&["Hunter", "Paladin"], &["Warlock", "Shaman"]),
    (&["Rogue", "Druid"], &["Hunter", "Priest"]),
    (&["Warlock", "Priest"], &["Warrior", "Paladin"]),
    (&["Warrior", "Mage"], &["Rogue", "Warlock"]),
];
const MAPS: &[&str] = &["PillaredArena", "BasicArena"];
const SEEDS: std::ops::Range<u64> = 1..49;

fn config(
    team1: &[&str],
    team2: &[&str],
    map: &str,
    seed: u64,
    burst: bool,
) -> HeadlessMatchConfig {
    // Burst graduated: its control excepts it everywhere, on both teams.
    let withheld = if burst {
        BTreeMap::new()
    } else {
        BTreeMap::from([("Burst".to_string(), every_exception_entry(PlayId::Burst))])
    };
    HeadlessMatchConfig {
        team1: team1.iter().map(|s| s.to_string()).collect(),
        team2: team2.iter().map(|s| s.to_string()).collect(),
        map: map.to_string(),
        random_seed: Some(seed),
        ai_profile: Some("TeamPlan".to_string()),
        team1_exceptions: withheld.clone(),
        team2_exceptions: withheld,
        ..Default::default()
    }
}

/// One `kill_window` and what came of it.
#[derive(Debug)]
struct Window {
    answered: bool,
    /// Answered by a burst chosen within one global cooldown of the onset.
    prompt: bool,
    /// The window had a known `until` (a lock) at its onset.
    timed: bool,
    killed: bool,
    /// Burst-step choices (`play_step == "burst"`) inside the window, and
    /// casts refused for landing after it.
    burst_steps: usize,
    refused: usize,
    /// The DPS decisions of the window's team made inside it that acted, and
    /// how many of them were a burst tool on the subject landing inside it.
    decisions: usize,
    chosen: usize,
    responses: usize,
}

/// The frame observation taken after the update that recorded `event` (the
/// trace's frame counter runs one update behind the observations).
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

fn vec3(v: &serde_json::Value) -> bevy::prelude::Vec3 {
    bevy::prelude::Vec3::new(
        v[0].as_f64().unwrap() as f32,
        v[1].as_f64().unwrap() as f32,
        v[2].as_f64().unwrap() as f32,
    )
}

/// Seconds from a cast's start to its landing: cast time, flight, and for
/// damage over time alone its first tick. Computed here, not by the
/// production fit, so the probe does not grade the fit with itself.
fn lands_after(defs: &AbilityDefinitions, ability: AbilityType, distance: f32) -> f32 {
    let def = defs.get(&ability).expect("a defined ability");
    let flight = def.projectile_speed.map_or(0.0, |speed| distance / speed);
    let dot_only = def.damage_base_max <= 0.0
        && def
            .applies_aura
            .as_ref()
            .is_some_and(|a| a.aura_type == AuraType::DamageOverTime);
    let first_tick = if dot_only {
        def.applies_aura.as_ref().unwrap().tick_interval
    } else {
        0.0
    };
    def.cast_time + flight + first_tick
}

/// An open window, as the trace builds it.
struct Open {
    team: u64,
    subject: u64,
    since: f32,
    until: Option<f32>,
    timed: bool,
}

/// Every `kill_window` of one played match.
fn windows(played: &Played, defs: &AbilityDefinitions) -> Vec<Window> {
    let frames = &played.frames;
    let entity_by_index: BTreeMap<u64, bevy::prelude::Entity> = frames[0]
        .combatants
        .keys()
        .map(|e| (e.index() as u64, *e))
        .collect();
    let mut open: BTreeMap<u64, Open> = BTreeMap::new();
    let mut out = Vec::new();
    let close = |w: Open, end: f32, end_frame: usize, out: &mut Vec<Window>| {
        let subject = entity_by_index[&w.subject];
        let inside = |e: &&serde_json::Value| {
            let t = e["sim_time"].as_f64().unwrap() as f32;
            e["kind"] == "ability_decision"
                && t >= w.since
                && t < end
                && e["actor"]["team"].as_u64() == Some(w.team)
        };
        let burst_steps = played
            .trace
            .iter()
            .filter(inside)
            .filter(|e| e["situation"]["play_step"] == "burst")
            .count();
        let refused = played
            .trace
            .iter()
            .filter(inside)
            .flat_map(|e| e["candidates"].as_array().into_iter().flatten())
            .filter(|c| c["reason"]["OutlastsWindow"].is_object())
            .count();
        // A burst tool chosen on the subject inside the window: when, and
        // whether it lands inside the window.
        let burst = |e: &serde_json::Value| -> Option<(f32, bool)> {
            if e["kind"] != "ability_decision" || e["outcome"]["type"] != "action_taken" {
                return None;
            }
            let t = e["sim_time"].as_f64().unwrap() as f32;
            let actor = &e["actor"];
            if t < w.since || t >= end || actor["team"].as_u64() != Some(w.team) {
                return None;
            }
            if e["outcome"]["target_id"].as_u64() != Some(w.subject) {
                return None;
            }
            let class = class_of(actor["class"].as_str().unwrap());
            let ability = e["outcome"]["ability"].as_str().and_then(ability_of)?;
            if !burst_tools(class).contains(&ability) {
                return None;
            }
            let f = frame_of(e).min(frames.len() - 1);
            let distance = frames[f].combatants[&subject]
                .position
                .distance(vec3(&actor["position"]));
            Some((
                t,
                t + lands_after(defs, ability, distance) <= w.until.unwrap_or(end) + 1e-3,
            ))
        };
        let starts: Vec<f32> = played
            .trace
            .iter()
            .filter_map(burst)
            .filter(|(_, landed)| *landed)
            .map(|(t, _)| t)
            .collect();
        let acting: Vec<&serde_json::Value> = played
            .trace
            .iter()
            .filter(inside)
            .filter(|e| e["outcome"]["type"] == "action_taken")
            .filter(|e| {
                e["actor"]["class"]
                    .as_str()
                    .is_some_and(|c| !burst_tools(class_of(c)).is_empty())
            })
            .collect();
        let decisions = acting.len();
        let chosen = acting.iter().filter(|e| burst(e).is_some()).count();
        let responses = acting
            .iter()
            .filter(|e| burst(e).is_some_and(|(_, landed)| landed))
            .count();
        let answered = !starts.is_empty();
        let prompt = starts.iter().any(|t| *t <= w.since + ONE_GCD);
        let f = end_frame.min(frames.len() - 1);
        let killed = !frames[f].combatants[&subject].alive;
        out.push(Window {
            answered,
            prompt,
            timed: w.timed,
            killed,
            burst_steps,
            refused,
            decisions,
            chosen,
            responses,
        });
    };
    for e in &played.trace {
        if e["kind"] != "situation_change" || e["name"] != "kill_window" {
            continue;
        }
        let team = e["team"].as_u64().unwrap();
        let t = e["sim_time"].as_f64().unwrap() as f32;
        let until = e["until"].as_f64().map(|u| u as f32);
        match e["change"].as_str().unwrap() {
            "onset" => {
                if let Some(w) = open.remove(&team) {
                    close(w, t, frame_of(e), &mut out);
                }
                open.insert(
                    team,
                    Open {
                        team,
                        subject: e["subject"].as_u64().unwrap(),
                        since: t,
                        until,
                        timed: until.is_some(),
                    },
                );
            }
            "extended" => {
                if let Some(w) = open.get_mut(&team) {
                    w.until = until;
                }
            }
            "end" => {
                if let Some(w) = open.remove(&team) {
                    close(w, t, frame_of(e), &mut out);
                }
            }
            other => panic!("kill_window change {other}"),
        }
    }
    // A window open at the match's end closes with it.
    let last = played
        .trace
        .last()
        .map_or(0.0, |e| e["sim_time"].as_f64().unwrap() as f32);
    for (_, w) in open {
        close(w, last + 1e-3, frames.len() - 1, &mut out);
    }
    out
}

/// The bank's windows under one arm.
fn bank(burst: bool) -> Vec<Window> {
    let defs = load_ability_definitions().expect("abilities.ron loads");
    let mut all = Vec::new();
    for (team1, team2) in COMPS {
        for map in MAPS {
            for seed in SEEDS {
                let played = play_seeded(config(team1, team2, map, seed, burst));
                all.extend(windows(&played, &defs));
            }
        }
    }
    all
}

struct Summary {
    windows: usize,
    answered: usize,
    prompt: usize,
    killed: usize,
    decisions: usize,
    chosen: usize,
    responses: usize,
}

fn summarise(label: &str, w: &[Window]) -> Summary {
    let n = w.len().max(1) as f64;
    let s = Summary {
        windows: w.len(),
        answered: w.iter().filter(|w| w.answered).count(),
        prompt: w.iter().filter(|w| w.prompt).count(),
        killed: w.iter().filter(|w| w.killed).count(),
        decisions: w.iter().map(|w| w.decisions).sum(),
        chosen: w.iter().map(|w| w.chosen).sum(),
        responses: w.iter().map(|w| w.responses).sum(),
    };
    let timed: Vec<&Window> = w.iter().filter(|w| w.timed).collect();
    eprintln!(
        "{label}: {} kill windows ({} with a known end), {} answered by a burst landing \
         inside ({:.1}%), {} of them within one GCD of the onset ({:.1}%), {} kills secured \
         inside ({:.1}%); timed windows: {} answered, {} kills; {} burst steps, {} casts \
         refused for outlasting a window; of {} DPS decisions inside a window, {} chose a \
         burst on its subject ({:.1}%) and {} of those landed inside it ({:.1}%)",
        s.windows,
        timed.len(),
        s.answered,
        100.0 * s.answered as f64 / n,
        s.prompt,
        100.0 * s.prompt as f64 / n,
        s.killed,
        100.0 * s.killed as f64 / n,
        timed.iter().filter(|w| w.answered).count(),
        timed.iter().filter(|w| w.killed).count(),
        w.iter().map(|w| w.burst_steps).sum::<usize>(),
        w.iter().map(|w| w.refused).sum::<usize>(),
        s.decisions,
        s.chosen,
        100.0 * s.chosen as f64 / s.decisions.max(1) as f64,
        s.responses,
        100.0 * s.responses as f64 / s.chosen.max(1) as f64,
    );
    s
}

/// A two-proportion z, `k1/n1` over `k2/n2` (pooled).
fn z(k1: usize, n1: usize, k2: usize, n2: usize) -> f64 {
    let (k1, n1, k2, n2) = (k1 as f64, n1 as f64, k2 as f64, n2 as f64);
    let p = (k1 + k2) / (n1 + n2);
    if p <= 0.0 || p >= 1.0 {
        return 0.0;
    }
    (k1 / n1 - k2 / n2) / (p * (1.0 - p) * (1.0 / n1 + 1.0 / n2)).sqrt()
}

/// Inside the kill windows, the play must turn significantly MORE of its
/// DPS's decisions into a burst on the window's subject than the same bank
/// without it (z of at least [`RESPONSE_Z`]) — the tier-4 response rate, read
/// per decision as the design's own example is — and must do no harm: no
/// fewer of those decisions may be a burst that LANDS inside the window, no
/// fewer windows may be answered by one, and no fewer kills may be secured
/// inside them (z above [`HARM_Z`] on each). All relative to the paired bank,
/// in units of its own noise, never a fitted number.
///
/// The window-level `answered` rate is held to no harm, not to a rise: it does
/// not move (the findings note, `docs/design/balance/2026-10-12-as275-burst-play.md`).
/// In both arms the DPS's ordinary chain already lands the same tools on the
/// same low enemy in about 38% of windows; what the play changes is how many
/// of a window's global cooldowns go to it.
#[test]
#[ignore = "plays 2,304 full matches; run with --release -- --ignored"]
fn burst_spends_more_kill_window_decisions_on_the_subject_and_does_no_harm() {
    let with = summarise("burst", &bank(true));
    let without = summarise("withheld", &bank(false));
    assert!(
        with.windows >= MIN_WINDOWS && without.windows >= MIN_WINDOWS,
        "the bank is thin: {} and {} kill windows (need {MIN_WINDOWS} in each) — add comps \
         or seeds, the behaviour is not what failed",
        with.windows,
        without.windows
    );
    let cz = z(
        with.chosen,
        with.decisions,
        without.chosen,
        without.decisions,
    );
    let rz = z(
        with.responses,
        with.decisions,
        without.responses,
        without.decisions,
    );
    let az = z(
        with.answered,
        with.windows,
        without.answered,
        without.windows,
    );
    let kz = z(with.killed, with.windows, without.killed, without.windows);
    let pz = z(with.prompt, with.windows, without.prompt, without.windows);
    eprintln!(
        "chosen z {cz:.2}; landed z {rz:.2}; answered z {az:.2}; within one GCD z {pz:.2}; \
         kills secured z {kz:.2}"
    );
    assert!(
        cz >= RESPONSE_Z,
        "the play does not spend significantly more kill-window decisions on a burst at the \
         window's subject: {}/{} vs {}/{} (z {cz:.2})",
        with.chosen,
        with.decisions,
        without.chosen,
        without.decisions
    );
    for (what, zv, k1, n1, k2, n2) in [
        (
            "kill-window decisions that land a burst inside the window",
            rz,
            with.responses,
            with.decisions,
            without.responses,
            without.decisions,
        ),
        (
            "kill windows answered by a landed burst",
            az,
            with.answered,
            with.windows,
            without.answered,
            without.windows,
        ),
        (
            "kills secured inside a window",
            kz,
            with.killed,
            with.windows,
            without.killed,
            without.windows,
        ),
    ] {
        assert!(
            zv > HARM_Z,
            "the play does harm — significantly fewer {what}: {k1}/{n1} vs {k2}/{n2} (z {zv:.2})"
        );
    }
}
