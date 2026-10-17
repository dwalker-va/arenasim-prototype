//! Characterization probe for the "mana charged only on successful cast
//! completion" fix (`combat_core/casting.rs`).
//!
//! Scenario: Mage+Priest vs Warrior+Shaman on TwinPillars. When the Warrior
//! dies first, the rest of the match is the Mage+Priest grinding down a lone
//! Shaman around the pillars, and the Shaman jukes the Mage's Frostbolts behind
//! cover — the probe judges every match of a seed bank ([`SEED_BANK`]) in which
//! that happened.
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

use arenasim::headless::{run_headless_match_observed, HeadlessMatchConfig, MatchResult};
use arenasim::CharacterClass;

/// The seeds the probe plays. It judges the matches in which the situation
/// it is about occurs — the Warrior dies first, and the lone Shaman jukes at
/// least one of the Mage's Frostbolts — and requires [`MIN_JUKED_MATCHES`] of
/// them, so a change that moves the RNG stream moves WHICH seeds qualify, not
/// whether the probe judges anything. (A single pinned seed carried this probe
/// until it went stale twice: AS-203 moved the Priest's path, and AS-282 the
/// Mage's Ice Barrier timing under a Frost lockout.)
const SEED_BANK: std::ops::Range<u64> = 0..90;

/// The fewest juked 2v1 matches the bank must hold (8 on AS-282's `main`).
const MIN_JUKED_MATCHES: usize = 4;

/// One per-frame sample of the state this probe cares about. The 2v1 window is
/// found positionally (frames after the Warrior's last-alive frame); the time
/// stamp places the first juke in it.
struct Frame {
    sim_time: f32,
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
    /// The Mage's lowest mana fraction over those frames, from the first
    /// Frostbolt the Shaman juked in them on: the casts the fix is about. A
    /// Mage that spent itself dry on casts that LANDED before any juke (and
    /// was refilling when the jukes began) says nothing about them.
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
            sim_time: frame.sim_time,
            mage_mana_pct: mage.map(|m| m.current_mana / m.max_mana),
            mage_alive: mage.map(|m| m.alive).unwrap_or(false),
            warrior_alive,
            shaman_alive,
        });
    })
    .expect("observed headless match failed");
    let log = std::fs::read_to_string(&log_path).unwrap_or_default();
    let _ = std::fs::remove_file(&log_path);

    let death_t = log
        .lines()
        .find(|l| l.contains("[DEATH]") && l.contains("Team 2 Warrior"))
        .and_then(log_time);
    let window_juke_times: Vec<f32> = death_t.map_or(Vec::new(), |d| {
        log.lines()
            .filter(|l| {
                l.contains("Team 1 Mage")
                    && l.contains("fails to cast Frostbolt")
                    && l.contains("line of sight")
            })
            .filter_map(log_time)
            .filter(|t| *t >= d)
            .collect()
    });
    let window_juke_fizzles = window_juke_times.len();
    let first_juke = window_juke_times.first().copied().unwrap_or(f32::INFINITY);

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

    Measured {
        result,
        window_frames: warrior_died.then_some(window.len()),
        min_window_mana: min_mana(&mut window.iter().copied().filter(|f| f.sim_time >= first_juke)),
        min_overall_mana: min_mana(&mut frames.iter()),
        window_juke_fizzles,
    }
}

/// Whether `m` is a match the probe is about: the Warrior died, a sustained
/// lone-Shaman 2v1 with the Mage alive followed, and the Shaman juked at least
/// one Frostbolt in it — so the mana floor is measured across the casts the fix
/// is about.
fn juked_2v1(m: &Measured) -> bool {
    m.window_frames.is_some_and(|w| w > 300) && m.window_juke_fizzles >= 1
}

/// (1) The Mage does not bankrupt itself on juked Frostbolts. In every match
/// of the bank where the lone Shaman juked the Mage (every frame of the 2v1
/// from the first juke on), the Mage's mana stays well above the near-zero
/// floor the bug produced. An activity guard proves the floor isn't trivially satisfied by a
/// Mage sitting at full mana.
#[test]
fn mage_mana_survives_juked_frostbolts() {
    let juked: Vec<(u64, Measured)> = SEED_BANK
        .map(|seed| (seed, measure(seed)))
        .filter(|(_, m)| juked_2v1(m))
        .collect();

    // Vacuity guard: the situation occurs, in enough matches to be a sample.
    assert!(
        juked.len() >= MIN_JUKED_MATCHES,
        "only {} of the bank's matches held a juked lone-Shaman 2v1 (seeds {:?}); \
         need {MIN_JUKED_MATCHES}",
        juked.len(),
        juked.iter().map(|(s, _)| s).collect::<Vec<_>>()
    );

    for (seed, m) in &juked {
        // Load-bearing assertion: the Mage's mana never collapses to near-zero
        // during the 2v1. The buggy build hit 0.004 here; 0.04 is ten times that.
        assert!(
            m.min_window_mana > 0.04,
            "seed {seed}: Mage mana collapsed during the 2v1 (min mana_pct = {:.3}); the \
             fizzle-drain bug is back — juked Frostbolts are charging mana again",
            m.min_window_mana
        );
        // Duration sanity only (this endgame is dampening-gated, not
        // mana-gated — see module docs). Do NOT assert a speedup.
        assert!(
            m.result.match_time < 200.0,
            "seed {seed} should still resolve well under the cap (got {:.1}s)",
            m.result.match_time
        );
    }

    // Activity guard: in some juked match the Mage IS spending mana under
    // pressure (it dips well below full), so the floor above is a real
    // "sustained, not idle" band — not trivially true because it never cast.
    assert!(
        juked.iter().any(|(_, m)| m.min_overall_mana < 0.5),
        "no juked match saw the Mage spend below half its mana: {:?}",
        juked
            .iter()
            .map(|(s, m)| (s, m.min_overall_mana))
            .collect::<Vec<_>>()
    );
}

/// The bank, match by match. Ignored by default: run it to see which seeds
/// hold the situation [`mage_mana_survives_juked_frostbolts`] judges.
#[test]
#[ignore]
fn scan_juke_mana_seeds() {
    for seed in SEED_BANK {
        let m = measure(seed);
        let good = juked_2v1(&m);
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
            if good { " <-- juked 2v1" } else { "" },
        );
    }
}
