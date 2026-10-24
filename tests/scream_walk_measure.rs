//! AS-388 measurement (ignored; prints, asserts nothing): what the `TeamPlan`
//! Priest's Psychic Scream does, and how its posture time splits.
//!
//! Two banks, the team-1 Priest under `TeamPlan`:
//!
//! - **camp**: Warrior+Priest against Warlock+Priest on PillaredArena, seeds
//!   1-12, both teams `TeamPlan` (`tests/camp_sweep.rs`'s comp);
//! - **cells**: every DPS+Priest (`TeamPlan`) against DPS+{Priest, Shaman,
//!   Druid, Paladin} (`Legacy`), on BasicArena and PillaredArena, seeds 1-4.
//!
//! Per bank: Psychic Screams by path (the Peel step, the retired dip's cast,
//! the defensive self-peel), whom they feared (enemy healers, enemy DPS),
//! Peel walks begun, dips entered, walks the scream gate held, and the
//! Priest's time per posture from the gates. Reads only the trace's JSON and
//! the combat log, so it runs unchanged on a build without the Peel walk (the
//! before arm).
//!
//! ```text
//! cargo test --release --test scream_walk_measure -- --ignored --nocapture
//! ```

mod common;

use std::collections::BTreeMap;

use arenasim::headless::HeadlessMatchConfig;
use common::staged::{play_seeded, sim_time, Played};

const HEALERS: [&str; 4] = ["Priest", "Shaman", "Druid", "Paladin"];
const DPS: [&str; 5] = ["Warrior", "Rogue", "Mage", "Warlock", "Hunter"];

#[derive(Default, Debug)]
struct Tally {
    matches: usize,
    wins: usize,
    screams_peel: usize,
    screams_dip: usize,
    screams_defensive: usize,
    feared_healer: usize,
    feared_dps: usize,
    /// Peel Screams that feared an enemy healer.
    peel_caught_healer: usize,
    peel_walks: usize,
    /// Peel walks followed by a Peel Scream within 8s.
    peel_walks_screamed: usize,
    dips: usize,
    dip_completes: usize,
    held: usize,
    posture_secs: BTreeMap<String, f32>,
}

fn tally(t: &mut Tally, played: &Played) {
    t.matches += 1;
    t.wins += (played.result.winner == Some(1)) as usize;
    let priest: Vec<&serde_json::Value> = played
        .trace
        .iter()
        .filter(|e| e["actor"]["team"] == 1 && e["actor"]["class"] == "Priest")
        .collect();
    let moves: Vec<&&serde_json::Value> = priest
        .iter()
        .filter(|e| e["kind"] == "movement_decision")
        .collect();
    let trig = |name: &str| moves.iter().filter(|e| e["trigger"] == name).count();
    t.dips += trig("DipEnter");
    t.dip_completes += trig("DipComplete");
    let walk_times: Vec<f32> = moves
        .iter()
        .filter(|e| e["trigger"] == "PeelWalk")
        .map(|e| sim_time(e))
        .collect();
    t.peel_walks += walk_times.len();
    let dip_complete_times: Vec<f32> = moves
        .iter()
        .filter(|e| e["trigger"] == "DipComplete")
        .map(|e| sim_time(e))
        .collect();

    // Screams, in order, by path.
    let mut paths = Vec::new();
    for e in priest.iter().filter(|e| e["kind"] == "ability_decision") {
        for c in e["candidates"].as_array().into_iter().flatten() {
            if c["ability"] != "PsychicScream" {
                continue;
            }
            if c["status"] == "chosen" {
                let at = sim_time(e);
                let path = if e["situation"]["play_step"] == "peel" {
                    "peel"
                } else if dip_complete_times.iter().any(|d| (d - at).abs() < 0.02) {
                    "dip"
                } else {
                    "defensive"
                };
                paths.push((at, path));
            } else if c["reason"]["PreconditionUnmet"]["note"]
                .as_str()
                .is_some_and(|n| n.contains("held"))
            {
                t.held += 1;
            }
        }
    }
    for &(_, p) in &paths {
        match p {
            "peel" => t.screams_peel += 1,
            "dip" => t.screams_dip += 1,
            _ => t.screams_defensive += 1,
        }
    }
    t.peel_walks_screamed += walk_times
        .iter()
        .filter(|&&w| {
            paths
                .iter()
                .any(|&(s, p)| p == "peel" && s >= w && s <= w + 8.0)
        })
        .count();

    // Victims, grouped by the cast they came from (one stamp per cast).
    let mut groups: Vec<(String, Vec<String>)> = Vec::new();
    for line in played.log.lines() {
        const FEARS: &str = "'s Psychic Scream fears ";
        let (Some(caster), Some(i)) = (line.find("Team 1 Priest #"), line.find(FEARS)) else {
            continue;
        };
        if caster > i || !line.contains("[CC]") {
            continue;
        }
        let stamp = line[..caster].to_string();
        let victim = line[i + FEARS.len()..].to_string();
        match groups.last_mut() {
            Some((s, v)) if *s == stamp => v.push(victim),
            _ => groups.push((stamp, vec![victim])),
        }
    }
    for (k, (_, victims)) in groups.iter().enumerate() {
        let path = paths.get(k).map_or("?", |p| p.1);
        let mut healer = false;
        for v in victims {
            if HEALERS.iter().any(|h| v.contains(&format!(" {h} #"))) {
                t.feared_healer += 1;
                healer = true;
            } else {
                t.feared_dps += 1;
            }
        }
        if path == "peel" && healer {
            t.peel_caught_healer += 1;
        }
    }

    // Posture time from the gates (combat clock 0) to the last event.
    let end = played.trace.iter().map(sim_time).fold(0.0f32, f32::max);
    let mut posture = "free".to_string();
    let mut since = 0.0f32;
    for e in moves.iter().filter(|e| !e["previous_posture"].is_null()) {
        let at = sim_time(e).max(0.0);
        *t.posture_secs.entry(posture.clone()).or_default() += at - since;
        posture = e["posture"].as_str().unwrap_or("?").to_string();
        since = at;
    }
    *t.posture_secs.entry(posture).or_default() += (end - since).max(0.0);
}

fn report(name: &str, t: &Tally) {
    let n = t.matches.max(1) as f32;
    println!("\n== {name}: {} matches, team 1 won {}", t.matches, t.wins);
    println!(
        "screams: peel {} / dip {} / defensive {}; feared: enemy healers {}, enemy DPS {}; peel screams that caught a healer {}",
        t.screams_peel, t.screams_dip, t.screams_defensive, t.feared_healer, t.feared_dps, t.peel_caught_healer
    );
    println!(
        "peel walks {} (screamed within 8s: {}); dips entered {} (completed {}); walks/dips held by the gate (decision ticks) {}",
        t.peel_walks, t.peel_walks_screamed, t.dips, t.dip_completes, t.held
    );
    let posture: Vec<String> = t
        .posture_secs
        .iter()
        .map(|(k, v)| format!("{k} {:.1}s", v / n))
        .collect();
    println!("posture per match from the gates: {}", posture.join(" / "));
}

fn config(
    team1: &[&str],
    team2: &[&str],
    map: &str,
    seed: u64,
    team2_profile: &str,
) -> HeadlessMatchConfig {
    HeadlessMatchConfig {
        team1: team1.iter().map(|s| s.to_string()).collect(),
        team2: team2.iter().map(|s| s.to_string()).collect(),
        map: map.into(),
        team1_ai_profile: Some("TeamPlan".into()),
        team2_ai_profile: Some(team2_profile.into()),
        random_seed: Some(seed),
        ..Default::default()
    }
}

#[test]
#[ignore]
fn scream_walk_mechanism() {
    let mut camp = Tally::default();
    for seed in 1..=12 {
        let played = play_seeded(config(
            &["Warrior", "Priest"],
            &["Warlock", "Priest"],
            "PillaredArena",
            seed,
            "TeamPlan",
        ));
        tally(&mut camp, &played);
    }
    report(
        "camp (Warrior+Priest v Warlock+Priest, Nagrand, both TeamPlan)",
        &camp,
    );

    for map in ["BasicArena", "PillaredArena"] {
        let mut cells = Tally::default();
        for dps in DPS {
            for healer in HEALERS {
                let enemy = DPS[(DPS.iter().position(|d| *d == dps).unwrap() + 2) % DPS.len()];
                for seed in 1..=4 {
                    let played = play_seeded(config(
                        &[dps, "Priest"],
                        &[enemy, healer],
                        map,
                        seed,
                        "Legacy",
                    ));
                    tally(&mut cells, &played);
                }
            }
        }
        report(
            &format!("cells, {map} (DPS+Priest TeamPlan v DPS+healer Legacy)"),
            &cells,
        );
    }
}
