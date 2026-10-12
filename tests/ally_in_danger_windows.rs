//! The `ally_in_danger` window's debounce, measured over played matches
//! (`situation::ally_in_danger`; findings in
//! `docs/design/balance/2026-10-12-as288-ally-in-danger-debounce.md`).
//!
//! A SEED BANK of 2v2 matches on both maps under both AI profiles. Every
//! window is read off the decision trace's `situation_change` events — an
//! onset, then an end or another onset (a new subject) — and its length taken
//! in frames. The probe prints the distribution, and asserts the debounce's
//! one structural promise: a window ends sooner than
//! `shared.danger_release` only because its teammate died, or because another
//! teammate's window took its place. Before the debounce, a third of the
//! windows on some maps lasted one or two frames, ended by an enemy stepping
//! across a pressure radius.
//!
//! `#[ignore]`d: it plays 224 full matches (about a minute in release). Run
//! it with
//!
//! ```text
//! cargo test --release --test ally_in_danger_windows -- --ignored --nocapture
//! ```
//!
//! With `ALLY_IN_DANGER_CSV=<path>` it also writes one row per window.

mod common;

use std::collections::BTreeMap;
use std::io::Write;

use arenasim::headless::HeadlessMatchConfig;
use arenasim::states::play_match::movement_config::load_movement_config;
use common::staged::{play_seeded, Played};

/// The bank: the Peel probe's comps (`tests/peel_play_probe.rs`), plus the
/// replay the flicker was first seen in (Warrior+Druid v Hunter+Shaman).
const COMPS: &[(&[&str], &[&str])] = &[
    (&["Mage", "Priest"], &["Warrior", "Druid"]),
    (&["Warlock", "Shaman"], &["Rogue", "Priest"]),
    (&["Hunter", "Paladin"], &["Warrior", "Mage"]),
    (&["Rogue", "Druid"], &["Hunter", "Priest"]),
    (&["Warrior", "Shaman"], &["Rogue", "Warlock"]),
    (&["Paladin", "Mage"], &["Warrior", "Hunter"]),
    (&["Warrior", "Druid"], &["Hunter", "Shaman"]),
];
const MAPS: &[&str] = &["PillaredArena", "BasicArena"];
const PROFILES: &[&str] = &["Legacy", "TeamPlan"];
const SEEDS: std::ops::Range<u64> = 1..9;
/// Fewer closed windows than this and the bank is too thin to say.
const MIN_WINDOWS: usize = 200;
/// A window this many frames long or shorter is a flicker.
const SHORT_FRAMES: u64 = 2;

/// How a window closed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Close {
    /// An `end` event.
    End,
    /// Another teammate's onset took its place.
    Switch,
    /// Still open when the match ended.
    Open,
}

#[derive(Debug)]
struct Window {
    team: u64,
    subject: u64,
    onset_frame: u64,
    /// Frames from onset to close; `None` while still open.
    frames: Option<u64>,
    close: Close,
    /// The subject was dead by the frame the window closed.
    subject_died: bool,
}

fn windows(played: &Played) -> Vec<Window> {
    let alive_at = |frame: u64, subject: u64| {
        // The trace's frame counter runs one update behind the observations.
        let f = (frame as usize + 1).min(played.frames.len() - 1);
        played.frames[f]
            .combatants
            .iter()
            .find(|(e, _)| e.index() as u64 == subject)
            .is_some_and(|(_, c)| c.alive)
    };
    let mut open: BTreeMap<u64, (u64, u64)> = BTreeMap::new();
    let mut out = Vec::new();
    let close = |team: u64, (f0, subject): (u64, u64), f1: Option<u64>, how: Close| Window {
        team,
        subject,
        onset_frame: f0,
        frames: f1.map(|f1| f1 - f0),
        close: how,
        subject_died: f1.is_some_and(|f1| !alive_at(f1, subject)),
    };
    for e in played
        .trace
        .iter()
        .filter(|e| e["kind"] == "situation_change" && e["name"] == "ally_in_danger")
    {
        let team = e["team"].as_u64().expect("team");
        let frame = e["frame"].as_u64().expect("frame");
        let subject = e["subject"].as_u64().expect("subject");
        match e["change"].as_str() {
            Some("onset") => {
                if let Some(prev) = open.insert(team, (frame, subject)) {
                    out.push(close(team, prev, Some(frame), Close::Switch));
                }
            }
            Some("end") => {
                let prev = open.remove(&team).expect("an end closes an open window");
                out.push(close(team, prev, Some(frame), Close::End));
            }
            other => panic!("unexpected ally_in_danger change {other:?}"),
        }
    }
    for (team, prev) in open {
        out.push(close(team, prev, None, Close::Open));
    }
    out
}

#[test]
#[ignore = "plays 224 full matches; run with --release -- --ignored"]
fn a_danger_window_outlasts_its_release_unless_its_teammate_dies() {
    let shared = load_movement_config().expect("movement.ron loads").shared;
    // The release in whole frames, less one for the frame clock's rounding.
    let release_frames = (shared.danger_release * 60.0).round() as u64 - 1;
    let mut csv = std::env::var("ALLY_IN_DANGER_CSV").ok().map(|path| {
        let mut f = std::fs::File::create(path).expect("the CSV");
        writeln!(
            f,
            "profile,map,comp,seed,team,subject,onset_frame,frames,close,subject_died"
        )
        .unwrap();
        f
    });
    let mut violations = Vec::new();
    let mut closed_total = 0;
    for profile in PROFILES {
        for map in MAPS {
            let (mut matches, mut all, mut short) = (0, 0, 0);
            let mut lengths = Vec::new();
            for (ci, (team1, team2)) in COMPS.iter().enumerate() {
                for seed in SEEDS {
                    let played = play_seeded(HeadlessMatchConfig {
                        team1: team1.iter().map(|s| s.to_string()).collect(),
                        team2: team2.iter().map(|s| s.to_string()).collect(),
                        map: map.to_string(),
                        random_seed: Some(seed),
                        ai_profile: Some(profile.to_string()),
                        ..Default::default()
                    });
                    matches += 1;
                    for w in windows(&played) {
                        all += 1;
                        if let Some(f) = w.frames {
                            lengths.push(f);
                            short += usize::from(f <= SHORT_FRAMES);
                            if w.close == Close::End && !w.subject_died && f < release_frames {
                                violations.push(format!(
                                    "{profile} {map} comp {ci} seed {seed}: team {} subject {} \
                                     window at frame {} ended after {f} frames",
                                    w.team, w.subject, w.onset_frame
                                ));
                            }
                        }
                        if let Some(csv) = csv.as_mut() {
                            writeln!(
                                csv,
                                "{profile},{map},{ci},{seed},{},{},{},{},{:?},{}",
                                w.team,
                                w.subject,
                                w.onset_frame,
                                w.frames.map_or(String::new(), |f| f.to_string()),
                                w.close,
                                w.subject_died
                            )
                            .unwrap();
                        }
                    }
                }
            }
            closed_total += lengths.len();
            lengths.sort_unstable();
            eprintln!(
                "{profile:8} {map:13}: {matches} matches, {all} windows ({:.2}/match), \
                 {short} of {SHORT_FRAMES} frames or fewer ({:.1}%), median {} frames",
                all as f64 / matches as f64,
                100.0 * short as f64 / all.max(1) as f64,
                lengths.get(lengths.len() / 2).copied().unwrap_or(0),
            );
        }
    }
    assert!(
        closed_total >= MIN_WINDOWS,
        "the bank is thin: {closed_total} closed windows (need {MIN_WINDOWS}) — add comps or \
         seeds, the debounce is not what failed"
    );
    assert!(
        violations.is_empty(),
        "{} windows ended inside the release ({release_frames} frames) with their teammate \
         alive and no other window taking over:\n{}",
        violations.len(),
        violations.join("\n")
    );
}
