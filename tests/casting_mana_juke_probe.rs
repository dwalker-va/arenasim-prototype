//! Characterization probe for the "mana charged only on successful cast
//! completion" fix (`combat_core/casting.rs`).
//!
//! Reproducing scenario: Mage+Priest vs Warrior+Shaman on PillaredArena, seed 1.
//! The Warrior dies early (~t=36 gates-relative); the rest of the match is the
//! Mage+Priest grinding down a lone Shaman around the pillars. The Shaman jukes
//! the Mage's Frostbolts behind cover repeatedly.
//!
//! BEFORE the fix, every juked Frostbolt still cost full mana at completion, so
//! the Mage bankrupted itself — its mana collapsed to ~0 (measured min mana_pct
//! after the Warrior died: 0.004) and it fell back to wand-only chip. AFTER the
//! fix, juked casts cost nothing, so the Mage keeps its mana and keeps casting.
//!
//! RE-BASELINE (Lightning Bolt instant-strike change): team2's Shaman now casts
//! Lightning Bolt as an instant strike instead of a traveling projectile, so
//! seed 1's lone-Shaman 2v1 unfolds differently and the Mage's window mana floor
//! moved from 0.130 to 0.063. That is still ~16x the 0.004 bug floor; the
//! fizzle-drain fix itself is unchanged and independently proven by
//! casting_mana_charge.rs (juked Frostbolts still charge no mana). The window
//! guard was re-baselined from 0.08 to 0.04 to track the new deterministic
//! value while preserving its original guard-to-observed ratio.
//!
//! NOTE ON DURATION: this 2v1 endgame is DAMPENING-gated, not mana-gated — the
//! lone Shaman survives on healing until arena dampening ramps its healing to
//! zero (~t=115, dampening 25%), and the Shaman dies ~t=118 in BOTH the buggy
//! and fixed builds. So the fix does NOT shorten this match (before 118.1s /
//! after 118.9s); it restores the Mage's mana economy. The load-bearing
//! assertion here is therefore the mana trajectory, not the duration. Duration
//! is only sanity-bounded well under the cap.
//!
//! Observed via `run_headless_match_observed`, which is read-only by
//! construction and proven non-perturbing by the determinism battery.

use arenasim::headless::runner::EndReason;
use arenasim::headless::{run_headless_match_observed, HeadlessMatchConfig};
use arenasim::CharacterClass;

/// One per-frame sample of the state this probe cares about. The 2v1 window is
/// found positionally (frames after the Warrior's last-alive frame), so no time
/// stamp is needed here.
struct Frame {
    mage_mana_pct: Option<f32>,
    mage_alive: bool,
    warrior_alive: bool,
    shaman_alive: bool,
}

fn seed1_config() -> HeadlessMatchConfig {
    HeadlessMatchConfig {
        team1: vec!["Mage".into(), "Priest".into()],
        team2: vec!["Warrior".into(), "Shaman".into()],
        map: "TwinPillars".into(),
        random_seed: Some(1),
        max_duration_secs: 300.0,
        ..Default::default()
    }
}

/// (1) The Mage does not bankrupt itself on juked Frostbolts. Across the lone-
/// Shaman 2v1 (every frame after the Warrior dies), the Mage's mana stays well
/// above the near-zero floor the bug produced. An activity guard proves the
/// window isn't trivially satisfied by a Mage sitting at full mana.
#[test]
fn mage_mana_survives_juked_frostbolts_seed1() {
    let mut frames: Vec<Frame> = Vec::new();
    let result = run_headless_match_observed(seed1_config(), true, None, |frame| {
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

    // The match resolves decisively for Team 1 (the Mage+Priest).
    assert_eq!(result.winner, Some(1), "Team 1 should win seed 1");
    assert_eq!(result.end_reason, EndReason::Kill, "seed 1 should end by kill, not cap");

    // The 2v1 window exists: the Warrior dies while the Shaman is still up. Find
    // the last frame the Warrior was alive — the window is everything after it.
    let warrior_death_idx = frames
        .iter()
        .rposition(|f| f.warrior_alive)
        .expect("Warrior must be alive on at least one frame");
    let window: Vec<&Frame> = frames[warrior_death_idx + 1..]
        .iter()
        .filter(|f| f.shaman_alive && f.mage_alive)
        .collect();
    // Vacuity guard: the lone-Shaman 2v1 with the Mage alive actually occurred.
    assert!(
        window.len() > 300,
        "expected a sustained lone-Shaman 2v1 window with the Mage alive, got {} frames",
        window.len()
    );

    // Load-bearing assertion: the Mage's mana never collapses to near-zero during
    // the 2v1. Buggy build hit 0.004 here. After Lightning Bolt became an instant
    // strike (Shaman is team2 here), seed 1's 2v1 unfolds differently and the
    // Mage's window floor moved from ~0.130 to ~0.063 — still an order of
    // magnitude above the 0.004 bug floor. The fizzle-drain fix is intact (juked
    // Frostbolts still charge no mana — see casting_mana_charge.rs); the lower
    // floor is the harder fight, not the bug. Guard re-baselined to 0.04.
    let min_window_mana = window
        .iter()
        .filter_map(|f| f.mage_mana_pct)
        .fold(f32::INFINITY, f32::min);
    assert!(
        min_window_mana > 0.04,
        "Mage mana collapsed during the 2v1 (min mana_pct = {:.3}); the fizzle-drain \
         bug is back — juked Frostbolts are charging mana again",
        min_window_mana
    );

    // Activity guard: the Mage IS spending mana under pressure (it dips well below
    // full at some point), so the floor above is a real "sustained, not idle" band
    // — not trivially true because the Mage never cast.
    let min_overall_mana = frames
        .iter()
        .filter_map(|f| f.mage_mana_pct)
        .fold(f32::INFINITY, f32::min);
    assert!(
        min_overall_mana < 0.5,
        "expected the Mage to actively spend mana (min mana_pct {:.3} implies it barely cast)",
        min_overall_mana
    );

    // Duration sanity only (this endgame is dampening-gated, not mana-gated — see
    // module docs). Bound it well under the 300s cap; do NOT assert a speedup.
    assert!(
        result.match_time < 200.0,
        "seed 1 should still resolve well under the cap (got {:.1}s)",
        result.match_time
    );
}
