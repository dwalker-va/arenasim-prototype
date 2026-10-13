//! The crowd-control lifecycle probe: how much of the enemy team a side
//! LOCKS, scored per frame — the instrument `cc-compounds-not-adds` names for
//! chained crowd control, and the Chain play's tier-4 probe
//! (`docs/design/proposals/2026-10-11-ai-architecture.md` §5 step 7).
//!
//! Win rate is one bit per match and cannot see a chain. These metrics can,
//! each read off every frame of a played match (1/60s apiece) for the team on
//! the RECEIVING end:
//!
//! - **denied seconds** — member-seconds spent locked (`utils::is_incapacitating`:
//!   a stun, a fear or horror, a polymorph, an incapacitate, a cyclone);
//! - **healer-locked seconds** — seconds its healer spent locked;
//! - **counterplay-free seconds** — seconds with two or more of its members
//!   locked at once, one of them its healer: nothing on that side can answer.
//!   In a 2v2 that is the whole team;
//! - **longest healer lockout** — the longest unbroken run of the healer
//!   locked: a chain is the only thing that grows it past one crowd
//!   control's duration;
//! - **hand-offs** — the Chain play's own choices (`situation.play_step ==
//!   "chain"` with a cast) by the side doing the locking, and its global
//!   cooldowns HELD for one (the same step with no cast).
//!
//! It plays a bank of matches under `TeamPlan` in three arms per seed — the
//! play withheld from both teams (NN), team 1 chaining (CN), team 2 chaining
//! (NC) — and scores each chaining match against its NN twin from the
//! CHAINING side: what it inflicted on the other team with the play, less
//! what the same team inflicted without it. No rate is pooled across the two
//! assignments.
//!
//! `#[ignore]`d: it plays the bank's full matches (minutes in release).
//!
//! ```text
//! cargo test --release --test cc_lifecycle_probe -- --ignored --nocapture
//! ```

mod common;

use std::collections::BTreeMap;

use arenasim::headless::HeadlessMatchConfig;
use arenasim::states::play_match::utils::is_incapacitating;
use common::staged::{play_seeded, Played};

const COMPS: &[(&[&str], &[&str])] = &[
    (&["Mage", "Paladin"], &["Warrior", "Priest"]),
    (&["Rogue", "Paladin"], &["Mage", "Shaman"]),
    (&["Warlock", "Druid"], &["Rogue", "Priest"]),
    (&["Mage", "Druid"], &["Hunter", "Paladin"]),
    (&["Rogue", "Mage"], &["Warlock", "Priest"]),
    (&["Warlock", "Paladin"], &["Mage", "Druid"]),
    (
        &["Rogue", "Mage", "Priest"],
        &["Warrior", "Warlock", "Paladin"],
    ),
    (
        &["Warlock", "Mage", "Druid"],
        &["Rogue", "Hunter", "Priest"],
    ),
];
const MAPS: &[&str] = &["BasicArena", "PillaredArena"];
const SEEDS: std::ops::Range<u64> = 1..25;
const FRAME: f32 = 1.0 / 60.0;
const WORKERS: usize = 4;

/// Which team the Chain play is withheld from in an arm.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Arm {
    /// Withheld from both.
    Nn,
    /// Team 1 chains.
    Cn,
    /// Team 2 chains.
    Nc,
}

impl Arm {
    fn withheld(self, team: u8) -> Vec<String> {
        let chains = matches!((self, team), (Arm::Cn, 1) | (Arm::Nc, 2));
        if chains {
            Vec::new()
        } else {
            vec!["Chain".to_string()]
        }
    }
}

fn config(team1: &[&str], team2: &[&str], map: &str, seed: u64, arm: Arm) -> HeadlessMatchConfig {
    HeadlessMatchConfig {
        team1: team1.iter().map(|s| s.to_string()).collect(),
        team2: team2.iter().map(|s| s.to_string()).collect(),
        map: map.to_string(),
        random_seed: Some(seed),
        ai_profile: Some("TeamPlan".to_string()),
        team1_withheld_plays: arm.withheld(1),
        team2_withheld_plays: arm.withheld(2),
        ..Default::default()
    }
}

/// What one team suffered in one match, and what the other team's Chain play
/// did to it.
#[derive(Clone, Copy, Debug, Default)]
struct Inflicted {
    denied: f32,
    healer_locked: f32,
    counterplay_free: f32,
    longest_healer_lockout: f32,
    /// The longest unbroken lockout of any one member.
    longest_lockout: f32,
    /// The locking side's hand-offs (a chain step that cast).
    handoffs: usize,
    /// The locking side's global cooldowns held for a hand-off.
    holds: usize,
    /// The locking side won.
    won: bool,
}

/// What `victim` suffered in `played`.
fn inflicted_on(played: &Played, victim: u8) -> Inflicted {
    let mut out = Inflicted::default();
    let mut run = 0.0f32;
    let mut runs: BTreeMap<bevy::prelude::Entity, f32> = BTreeMap::new();
    for frame in played.frames.iter().filter(|f| f.gates_open) {
        let members: Vec<_> = frame
            .combatants
            .values()
            .filter(|c| c.team == victim && c.alive && !c.is_pet)
            .collect();
        let locked: Vec<_> = members
            .iter()
            .filter(|c| c.aura_types.iter().any(is_incapacitating))
            .collect();
        out.denied += FRAME * locked.len() as f32;
        for (e, c) in frame
            .combatants
            .iter()
            .filter(|(_, c)| c.team == victim && !c.is_pet)
        {
            let r = runs.entry(*e).or_default();
            if c.alive && c.aura_types.iter().any(is_incapacitating) {
                *r += FRAME;
                out.longest_lockout = out.longest_lockout.max(*r);
            } else {
                *r = 0.0;
            }
        }
        let healer_locked = locked.iter().any(|c| c.class.is_healer());
        if healer_locked {
            out.healer_locked += FRAME;
            run += FRAME;
            out.longest_healer_lockout = out.longest_healer_lockout.max(run);
        } else {
            run = 0.0;
        }
        if locked.len() >= 2 && healer_locked {
            out.counterplay_free += FRAME;
        }
    }
    let locker = 3 - victim as u64;
    for e in played.trace.iter().filter(|e| {
        e["kind"] == "ability_decision"
            && e["actor"]["team"].as_u64() == Some(locker)
            && e["situation"]["play_step"] == "chain"
    }) {
        if e["outcome"]["ability"].is_string() {
            out.handoffs += 1;
        } else {
            out.holds += 1;
        }
    }
    out.won = played.result.winner == Some(3 - victim);
    out
}

/// Why the locking side's chain steps did not hand off: for each decision
/// under the Chain play, the reason its chain step gave each hand-off tool
/// (the tool's FIRST candidate entry — the chain step runs first), keyed
/// `"<class> <tool>: <reason>"`.
fn chain_verdicts(played: &Played, locker: u64, tally: &mut BTreeMap<String, usize>) {
    for e in played.trace.iter().filter(|e| {
        e["kind"] == "ability_decision"
            && e["actor"]["team"].as_u64() == Some(locker)
            && e["situation"]["play"]["name"] == "chain"
    }) {
        let class = e["actor"]["class"].as_str().unwrap_or("?");
        let mut seen = Vec::new();
        for c in e["candidates"].as_array().into_iter().flatten() {
            let ability = c["ability"].as_str().unwrap_or("?").to_string();
            let is_tool = [
                "HammerOfJustice",
                "KidneyShot",
                "Fear",
                "DeathCoil",
                "Polymorph",
                "Cyclone",
            ]
            .contains(&ability.as_str());
            if !is_tool || seen.contains(&ability) {
                continue;
            }
            seen.push(ability.clone());
            let reason = match &c["reason"] {
                serde_json::Value::String(s) => s.clone(),
                serde_json::Value::Object(m) => {
                    let (k, v) = m.iter().next().expect("a reason");
                    match v["note"].as_str() {
                        Some(note) => format!("{k}({note})"),
                        None => k.clone(),
                    }
                }
                serde_json::Value::Null => format!("{}", c["status"]),
                other => other.to_string(),
            };
            *tally
                .entry(format!("{class} {ability}: {reason}"))
                .or_default() += 1;
        }
    }
}

/// One cell of the bank: a comp on a map at a seed, scored from one side.
#[derive(Clone, Debug)]
struct Pair {
    /// The chaining side's classes.
    chainer: Vec<String>,
    map: &'static str,
    /// Team 1 chains (CN) or team 2 (NC).
    team: u8,
    /// What the chaining side inflicted with the play, and without (NN).
    with: Inflicted,
    without: Inflicted,
    /// The chaining side's chain-step verdicts ([`chain_verdicts`]).
    verdicts: BTreeMap<String, usize>,
}

fn bank() -> Vec<Pair> {
    let mut cells = Vec::new();
    for (t1, t2) in COMPS {
        for map in MAPS {
            for seed in SEEDS {
                cells.push((*t1, *t2, *map, seed));
            }
        }
    }
    let chunks: Vec<_> = cells.chunks(cells.len().div_ceil(WORKERS)).collect();
    std::thread::scope(|scope| {
        let handles: Vec<_> = chunks
            .into_iter()
            .map(|chunk| {
                scope.spawn(move || {
                    let mut pairs = Vec::new();
                    for &(t1, t2, map, seed) in chunk {
                        let mut arms = BTreeMap::new();
                        for arm in [Arm::Nn, Arm::Cn, Arm::Nc] {
                            arms.insert(arm, play_seeded(config(t1, t2, map, seed, arm)));
                        }
                        for (arm, team, victim, side) in
                            [(Arm::Cn, 1u8, 2u8, t1), (Arm::Nc, 2, 1, t2)]
                        {
                            let mut verdicts = BTreeMap::new();
                            chain_verdicts(&arms[&arm], team as u64, &mut verdicts);
                            pairs.push(Pair {
                                verdicts,
                                chainer: side.iter().map(|s| s.to_string()).collect(),
                                map,
                                team,
                                with: inflicted_on(&arms[&arm], victim),
                                without: inflicted_on(&arms[&Arm::Nn], victim),
                            });
                        }
                    }
                    pairs
                })
            })
            .collect();
        handles
            .into_iter()
            .flat_map(|h| h.join().expect("a bank worker"))
            .collect()
    })
}

/// Mean, and the paired z of the mean difference (0 when every pair agrees).
fn paired(diffs: &[f64]) -> (f64, f64) {
    let n = diffs.len() as f64;
    if n < 2.0 {
        return (diffs.first().copied().unwrap_or(0.0), 0.0);
    }
    let mean = diffs.iter().sum::<f64>() / n;
    let var = diffs.iter().map(|d| (d - mean).powi(2)).sum::<f64>() / (n - 1.0);
    let z = if var > 0.0 {
        mean / (var / n).sqrt()
    } else {
        0.0
    };
    (mean, z)
}

/// The slice's row: per-match means without → with, the paired z, and the
/// win flips.
fn row(label: &str, pairs: &[&Pair]) -> BTreeMap<&'static str, (f64, f64, f64)> {
    let mut out = BTreeMap::new();
    let metrics: [(&'static str, fn(&Inflicted) -> f64); 6] = [
        ("counterplay_free", |i| i.counterplay_free as f64),
        ("healer_locked", |i| i.healer_locked as f64),
        ("denied", |i| i.denied as f64),
        ("longest_healer_lockout", |i| {
            i.longest_healer_lockout as f64
        }),
        ("longest_lockout", |i| i.longest_lockout as f64),
        ("win", |i| i.won as u8 as f64),
    ];
    let n = pairs.len();
    let handoffs: usize = pairs.iter().map(|p| p.with.handoffs).sum();
    let holds: usize = pairs.iter().map(|p| p.with.holds).sum();
    let gains = pairs
        .iter()
        .filter(|p| p.with.won && !p.without.won)
        .count();
    let losses = pairs
        .iter()
        .filter(|p| !p.with.won && p.without.won)
        .count();
    print!("{label:<34} n={n:>4} handoffs={handoffs:>4} holds={holds:>5} flips={gains}/{losses}");
    for (name, f) in metrics {
        let base = pairs.iter().map(|p| f(&p.without)).sum::<f64>() / n.max(1) as f64;
        let diffs: Vec<f64> = pairs.iter().map(|p| f(&p.with) - f(&p.without)).collect();
        let (delta, z) = paired(&diffs);
        print!("  {name} {base:.2}{delta:+.2} (z={z:+.2})");
        out.insert(name, (base, delta, z));
    }
    println!();
    out
}

/// How far above the withheld twins, in their own paired standard errors,
/// the chaining side's lockout must sit.
const RESPONSE_Z: f64 = 3.0;
/// How far below them, in paired standard errors, a measure may fall before
/// the play is doing harm.
const HARM_Z: f64 = -2.0;

/// The tier-4 assertions, against the SAME seeds with the play withheld (the
/// NN twins), in units of the bank's own noise — never against a fitted
/// number:
///
/// - the play hands off in the chaining arms, and never where it is
///   withheld from both teams;
/// - the chaining side locks the other team for longer: its denied seconds
///   and its longest unbroken lockout rise, each by `RESPONSE_Z` paired
///   standard errors or more;
/// - and it does no harm: counterplay-free seconds, the healer's locked
///   seconds and the chaining side's wins fall by no more than `HARM_Z`.
#[test]
#[ignore = "plays the bank's full matches; run with --release -- --ignored --nocapture"]
fn chain_locks_the_other_team_longer_and_does_no_harm() {
    let pairs = bank();
    let handoffs: usize = pairs.iter().map(|p| p.with.handoffs).sum();
    let withheld: usize = pairs
        .iter()
        .map(|p| p.without.handoffs + p.without.holds)
        .sum();
    assert!(handoffs > 0, "the bank never handed off");
    assert_eq!(withheld, 0, "a chain step with the play withheld");
    let all: Vec<&Pair> = pairs.iter().collect();
    let r = row("overall", &all);
    for name in ["denied", "longest_lockout"] {
        let (_, delta, z) = r[name];
        assert!(
            z >= RESPONSE_Z,
            "{name}: {delta:+.2}s a match (z={z:+.2}), not {RESPONSE_Z} standard errors up"
        );
    }
    for name in ["counterplay_free", "healer_locked", "win"] {
        let (_, delta, z) = r[name];
        assert!(
            z > HARM_Z,
            "{name}: {delta:+.2} (z={z:+.2}) — the play does harm"
        );
    }
}

#[test]
#[ignore = "plays the bank's full matches; run with --release -- --ignored --nocapture"]
fn report_chain_play_lockout() {
    let pairs = bank();
    let all: Vec<&Pair> = pairs.iter().collect();
    let mut verdicts: BTreeMap<String, usize> = BTreeMap::new();
    for p in &pairs {
        for (k, n) in &p.verdicts {
            *verdicts.entry(k.clone()).or_default() += n;
        }
    }
    let mut ranked: Vec<_> = verdicts.into_iter().collect();
    ranked.sort_by_key(|r| std::cmp::Reverse(r.1));
    println!(
        "
=== chain-step verdicts, decisions under the Chain play ==="
    );
    for (k, n) in ranked {
        println!("{n:>7}  {k}");
    }
    println!("\n=== the Chain play, scored from the chaining side (with - without) ===");
    row("overall", &all);
    for map in MAPS {
        for team in [1u8, 2] {
            let slice: Vec<&Pair> = pairs
                .iter()
                .filter(|p| p.map == *map && p.team == team)
                .collect();
            row(&format!("{map}, team {team} chains"), &slice);
        }
    }
    for class in ["Paladin", "Rogue", "Warlock", "Mage", "Druid"] {
        let slice: Vec<&Pair> = pairs
            .iter()
            .filter(|p| p.chainer.iter().any(|c| c == class))
            .collect();
        row(&format!("chaining side has {class}"), &slice);
    }
    for (t1, t2) in COMPS {
        for (side, team) in [(*t1, 1u8), (*t2, 2)] {
            let slice: Vec<&Pair> = pairs
                .iter()
                .filter(|p| {
                    p.team == team
                        && p.chainer
                            .iter()
                            .map(String::as_str)
                            .eq(side.iter().copied())
                })
                .collect();
            row(&format!("{} (team {team})", side.join("+")), &slice);
        }
    }
}
