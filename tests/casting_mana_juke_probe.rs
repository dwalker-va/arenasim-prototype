//! Characterization probe for the "mana charged only on successful cast
//! completion" fix (`combat_core/casting.rs`).
//!
//! Scenario: Mage+Priest vs Warrior+Shaman on TwinPillars, at [`SEED`]. The
//! Warrior dies early; the rest of the match is the Mage+Priest grinding down a
//! lone Shaman around the pillars, and the Shaman jukes the Mage's Frostbolts
//! behind cover repeatedly — the probe counts those fizzles and requires one.
//!
//! BEFORE the fix, every juked Frostbolt still cost full mana at completion, so
//! the Mage bankrupted itself — its mana collapsed to ~0 (measured min mana_pct
//! after the Warrior died: 0.004) and it fell back to wand-only chip. AFTER the
//! fix, juked casts cost nothing, so the Mage keeps its mana and keeps casting.
//!
//! NOTE ON DURATION: this 2v1 endgame is DAMPENING-gated, not mana-gated — the
//! lone Shaman survives on healing until arena dampening ramps its healing to
//! zero, so the fix does NOT shorten this match; it restores the Mage's mana
//! economy. The load-bearing assertion here is therefore the mana trajectory,
//! not the duration. Duration is only sanity-bounded well under the cap.
//!
//! Observed via `run_headless_match_observed`, which is read-only by
//! construction and proven non-perturbing by the determinism battery.

use arenasim::headless::runner::EndReason;
use arenasim::headless::{run_headless_match_observed, HeadlessMatchConfig, MatchResult};
use arenasim::CharacterClass;

/// The pinned match. Seed 1 carried this probe until AS-203 projected the
/// `Legacy` healer formation point out of pillars: the Priest's trajectory
/// changed and at 1 the Warrior no longer dies (team 2 wins at 53.0s). A
/// re-scan with `scan_juke_mana_seeds` also found that on `main` before that
/// change, seed 1's 2v1 held NO juked Frostbolt at all — its mana floor was
/// measured across casts the fix is not about — so the probe now requires
/// one. Seed 46: team-1 kill at 82.7s, a 2960-frame 2v1 with 18 Frostbolts
/// juked for line of sight, the Mage's mana floor 0.082 against the guard of
/// 0.04 (seed 1's floor was 0.063).
const SEED: u64 = 46;

/// One per-frame sample of the state this probe cares about. The 2v1 window is
/// found positionally (frames after the Warrior's last-alive frame), so no time
/// stamp is needed here.
struct Frame {
    mage_mana_pct: Option<f32>,
    mage_alive: bool,
    warrior_alive: bool,
    shaman_alive: bool,
}

fn config(seed: u64) -> HeadlessMatchConfig {
    HeadlessMatchConfig {
        team1: vec!["Mage".into(), "Priest".into()],
        team2: vec!["Warrior".into(), "Shaman".into()],
        map: "TwinPillars".into(),
        random_seed: Some(seed),
        max_duration_secs: 300.0,
        ..Default::default()
    }
}

/// What one match says about the Mage's mana through the lone-Shaman 2v1.
struct Measured {
    result: MatchResult,
    /// Frames after the Warrior's death with the Shaman and the Mage alive;
    /// `None` when the Warrior never died.
    window_frames: Option<usize>,
    /// The Mage's lowest mana fraction over those frames.
    min_window_mana: f32,
    /// The Mage's lowest mana fraction over the whole match.
    min_overall_mana: f32,
    /// The Mage's Frostbolts that fizzled for line of sight after the Warrior
    /// died — the juked casts the fix stops charging for.
    window_juke_fizzles: usize,
}

/// Parse a `[  12.34s] ...` leading timestamp off a combat-log line.
fn log_time(line: &str) -> Option<f32> {
    let open = line.find('[')?;
    let close = line[open..].find("s]")? + open;
    line[open + 1..close].trim().parse::<f32>().ok()
}

fn measure(seed: u64) -> Measured {
    // The combat log carries the fizzles; suppress_log gates output only, not
    // the sim, so writing it leaves the match unchanged.
    let log_tmp = tempfile::NamedTempFile::new().unwrap();
    let log_path = log_tmp.path().to_path_buf();
    drop(log_tmp);
    let mut cfg = config(seed);
    cfg.output_path = Some(log_path.to_string_lossy().into_owned());

    let mut frames: Vec<Frame> = Vec::new();
    let result = run_headless_match_observed(cfg, false, None, |frame| {
        // Resolve the three combatants of interest by (team, class). None are pets.
        let mut mage = None;
        let mut warrior_alive = false;
        let mut shaman_alive = false;
        for obs in frame.combatants.values() {
            if obs.is_pet {
                continue;
            }
            match (obs.team, obs.class) {
                (1, CharacterClass::Mage) => mage = Some(obs),
                (2, CharacterClass::Warrior) => warrior_alive = obs.alive,
                (2, CharacterClass::Shaman) => shaman_alive = obs.alive,
                _ => {}
            }
        }
        frames.push(Frame {
            mage_mana_pct: mage.map(|m| m.current_mana / m.max_mana),
            mage_alive: mage.map(|m| m.alive).unwrap_or(false),
            warrior_alive,
            shaman_alive,
        });
    })
    .expect("observed headless match failed");
    let log = std::fs::read_to_string(&log_path).unwrap_or_default();
    let _ = std::fs::remove_file(&log_path);

    // The 2v1 window: every frame after the Warrior's last alive one, while the
    // Shaman and the Mage are both up.
    let warrior_death_idx = frames.iter().rposition(|f| f.warrior_alive);
    let warrior_died = !frames.last().is_some_and(|f| f.warrior_alive);
    let window: Vec<&Frame> = match warrior_death_idx {
        Some(i) if warrior_died => frames[i + 1..]
            .iter()
            .filter(|f| f.shaman_alive && f.mage_alive)
            .collect(),
        _ => Vec::new(),
    };
    let min_mana = |it: &mut dyn Iterator<Item = &Frame>| {
        it.filter_map(|f| f.mage_mana_pct)
            .fold(f32::INFINITY, f32::min)
    };

    let death_t = log
        .lines()
        .find(|l| l.contains("[DEATH]") && l.contains("Team 2 Warrior"))
        .and_then(log_time);
    let window_juke_fizzles = death_t.map_or(0, |d| {
        log.lines()
            .filter(|l| {
                l.contains("Team 1 Mage")
                    && l.contains("fails to cast Frostbolt")
                    && l.contains("line of sight")
                    && log_time(l).is_some_and(|t| t >= d)
            })
            .count()
    });

    Measured {
        result,
        window_frames: warrior_died.then_some(window.len()),
        min_window_mana: min_mana(&mut window.iter().copied()),
        min_overall_mana: min_mana(&mut frames.iter()),
        window_juke_fizzles,
    }
}

/// (1) The Mage does not bankrupt itself on juked Frostbolts. Across the lone-
/// Shaman 2v1 (every frame after the Warrior dies), the Mage's mana stays well
/// above the near-zero floor the bug produced. An activity guard proves the
/// window isn't trivially satisfied by a Mage sitting at full mana.
#[test]
fn mage_mana_survives_juked_frostbolts() {
    let m = measure(SEED);
    let result = &m.result;

    // The match resolves decisively for Team 1 (the Mage+Priest).
    assert_eq!(result.winner, Some(1), "Team 1 should win seed {SEED}");
    assert_eq!(
        result.end_reason,
        EndReason::Kill,
        "seed {SEED} should end by kill, not cap"
    );

    // Vacuity guard: the lone-Shaman 2v1 with the Mage alive actually occurred.
    let window_frames = m
        .window_frames
        .expect("the Warrior must die, opening the lone-Shaman 2v1");
    assert!(
        window_frames > 300,
        "expected a sustained lone-Shaman 2v1 window with the Mage alive, got {window_frames} frames",
    );
    // Vacuity guard: the Shaman actually juked Frostbolts in that window, so
    // the mana floor below is measured across the casts the fix is about.
    assert!(
        m.window_juke_fizzles >= 1,
        "no Mage Frostbolt fizzled for line of sight in the 2v1 — nothing was juked",
    );

    // Load-bearing assertion: the Mage's mana never collapses to near-zero during
    // the 2v1. The buggy build hit 0.004 here; 0.04 is ten times that.
    assert!(
        m.min_window_mana > 0.04,
        "Mage mana collapsed during the 2v1 (min mana_pct = {:.3}); the fizzle-drain \
         bug is back — juked Frostbolts are charging mana again",
        m.min_window_mana
    );

    // Activity guard: the Mage IS spending mana under pressure (it dips well below
    // full at some point), so the floor above is a real "sustained, not idle" band
    // — not trivially true because the Mage never cast.
    assert!(
        m.min_overall_mana < 0.5,
        "expected the Mage to actively spend mana (min mana_pct {:.3} implies it barely cast)",
        m.min_overall_mana
    );

    // Duration sanity only (this endgame is dampening-gated, not mana-gated — see
    // module docs). Bound it well under the 300s cap; do NOT assert a speedup.
    assert!(
        result.match_time < 200.0,
        "seed {SEED} should still resolve well under the cap (got {:.1}s)",
        result.match_time
    );
}

/// Seed scan for re-pinning `SEED`. Ignored by default. A good pin is a
/// team-1 kill with a long lone-Shaman window in which the Shaman juked
/// several Frostbolts.
#[test]
#[ignore]
fn scan_juke_mana_seeds() {
    for seed in 0u64..60 {
        let m = measure(seed);
        let good = m.result.winner == Some(1)
            && m.result.end_reason == EndReason::Kill
            && m.window_frames.is_some_and(|w| w > 300)
            && m.window_juke_fizzles >= 1
            && m.min_window_mana > 0.04
            && m.min_overall_mana < 0.5
            && m.result.match_time < 200.0;
        eprintln!(
            "seed {seed:2}: winner={:?} t={:5.1} window={:>5} juked={:2} min_window_mana={:.3} \
             min_mana={:.3}{}",
            m.result.winner,
            m.result.match_time,
            m.window_frames
                .map(|w| w.to_string())
                .unwrap_or_else(|| "none".into()),
            m.window_juke_fizzles,
            m.min_window_mana,
            m.min_overall_mana,
            if good { " <-- GOOD" } else { "" },
        );
    }
}
