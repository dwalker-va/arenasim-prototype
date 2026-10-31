//! AS-296 measurement: how long the Warlock spends MANA-STARVED, over a bank
//! of matches — the number Life Tap exists to move.
//!
//! Ignored (minutes of simulation). It reads the bank from the `--batch`
//! JSONL at `LIFE_TAP_PROBE_JSONL` (`scripts/gen_sweep.py` writes one), plays
//! every config through the observed runner on `LIFE_TAP_PROBE_JOBS` threads
//! (default 3), and prints, summed over every Warlock's living post-gate
//! frames: the seconds below `STARVED_FRACTION` of its mana pool, the seconds
//! it could not afford a Shadow Bolt, and the taps (a frame on which its mana
//! rose by more than `TAP_JUMP`, which nothing else in the sim gives it).
//!
//! It reads nothing Life Tap added, so the same file measures the arm before
//! the ability existed: copy it into a checkout of the base commit and run it
//! there over the same JSONL.
//!
//! ```text
//! LIFE_TAP_PROBE_JSONL=/path/sweep.jsonl \
//!   cargo test --release --test life_tap_probe -- --ignored --nocapture
//! ```

use std::sync::{Arc, Mutex};

use arenasim::headless::{run_headless_match_observed, HeadlessMatchConfig};
use arenasim::states::match_config::CharacterClass;
use arenasim::states::play_match::ability_config::load_ability_definitions;
use arenasim::states::play_match::AbilityType;

/// The starved line `class_ai::warlock::LIFE_TAP_STARVED_FRACTION` draws,
/// restated so the probe compiles where that constant does not exist yet.
const STARVED_FRACTION: f32 = 0.3;
/// A one-frame mana rise larger than this is a tap.
const TAP_JUMP: f32 = 10.0;
const FRAME: f32 = 1.0 / 60.0;

#[derive(Default, Debug)]
struct Tally {
    matches: usize,
    warlocks: usize,
    alive_secs: f64,
    starved_secs: f64,
    no_bolt_secs: f64,
    taps: usize,
}

fn measure(config: HeadlessMatchConfig, bolt_cost: f32) -> Tally {
    let mut tally = Tally {
        matches: 1,
        ..Default::default()
    };
    let mut last_mana: std::collections::BTreeMap<bevy::prelude::Entity, f32> = Default::default();
    let mut seen: std::collections::BTreeSet<bevy::prelude::Entity> = Default::default();
    run_headless_match_observed(config, true, None, |frame| {
        if !frame.gates_open {
            return;
        }
        for (entity, c) in &frame.combatants {
            if c.is_pet || c.class != CharacterClass::Warlock || !c.alive {
                continue;
            }
            seen.insert(*entity);
            tally.alive_secs += FRAME as f64;
            if c.current_mana < c.max_mana * STARVED_FRACTION {
                tally.starved_secs += FRAME as f64;
            }
            if c.current_mana < bolt_cost {
                tally.no_bolt_secs += FRAME as f64;
            }
            if let Some(prev) = last_mana.insert(*entity, c.current_mana) {
                if c.current_mana - prev > TAP_JUMP {
                    tally.taps += 1;
                }
            }
        }
    })
    .expect("the match plays");
    tally.warlocks = seen.len();
    tally
}

#[test]
#[ignore = "measurement: minutes of simulation over a sweep's JSONL"]
fn warlock_mana_starved_time() {
    let path = std::env::var("LIFE_TAP_PROBE_JSONL").expect("set LIFE_TAP_PROBE_JSONL");
    let jobs: usize = std::env::var("LIFE_TAP_PROBE_JOBS")
        .ok()
        .and_then(|j| j.parse().ok())
        .unwrap_or(3);
    let bolt_cost = load_ability_definitions()
        .expect("abilities.ron loads")
        .get_unchecked(&AbilityType::Shadowbolt)
        .mana_cost;
    let configs: Vec<HeadlessMatchConfig> = std::fs::read_to_string(&path)
        .expect("the JSONL")
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).expect("a HeadlessMatchConfig"))
        .filter(|c: &HeadlessMatchConfig| {
            c.team1
                .iter()
                .chain(&c.team2)
                .any(|class| class == "Warlock")
        })
        .collect();
    let queue = Arc::new(Mutex::new(configs));
    let total = Arc::new(Mutex::new(Tally::default()));
    let workers: Vec<_> = (0..jobs)
        .map(|_| {
            let queue = queue.clone();
            let total = total.clone();
            std::thread::spawn(move || loop {
                let Some(config) = queue.lock().unwrap().pop() else {
                    return;
                };
                let t = measure(config, bolt_cost);
                let mut sum = total.lock().unwrap();
                sum.matches += t.matches;
                sum.warlocks += t.warlocks;
                sum.alive_secs += t.alive_secs;
                sum.starved_secs += t.starved_secs;
                sum.no_bolt_secs += t.no_bolt_secs;
                sum.taps += t.taps;
            })
        })
        .collect();
    for w in workers {
        w.join().expect("a worker");
    }
    let t = total.lock().unwrap();
    assert!(t.warlocks > 0, "the bank should field Warlocks");
    println!(
        "LIFE_TAP_PROBE matches={} warlocks={} alive_secs={:.1} starved_secs={:.1} ({:.1}% of alive) \
         no_bolt_secs={:.1} ({:.1}%) taps={} starved_per_warlock={:.2}s taps_per_warlock={:.3}",
        t.matches,
        t.warlocks,
        t.alive_secs,
        t.starved_secs,
        100.0 * t.starved_secs / t.alive_secs,
        t.no_bolt_secs,
        100.0 * t.no_bolt_secs / t.alive_secs,
        t.taps,
        t.starved_secs / t.warlocks as f64,
        t.taps as f64 / t.warlocks as f64,
    );
}
