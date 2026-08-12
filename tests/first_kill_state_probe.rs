//! Is the post-first-kill window a CAUSE of the result, or a symptom?
//!
//! The log read found that the match is won iff the off-target (the enemy Rogue)
//! dies, that ~90% of damage onto it lands *after* the called healer dies, and
//! that a single threshold on the remaining window separates 31 of 32 matches.
//!
//! That invites an obvious objection: perhaps a short window simply means we were
//! already losing. A team that is nearly dead when the enemy healer finally falls
//! has both a short remaining window *and* a loss, with the real cause upstream
//! of both. If so, "lengthen the window" is not a lever at all.
//!
//! This probe measures the state at the instant the enemy healer dies — our
//! team's health, the off-target's health, and who dies afterwards — and splits
//! it by result. Read-only, via `run_headless_match_observed`.
//!
//! ```bash
//! cargo test --release --test first_kill_state_probe -- --ignored --nocapture
//! ```

use arenasim::headless::{run_headless_match_observed, HeadlessMatchConfig};
use arenasim::states::match_config::CharacterClass;

#[derive(Debug, Clone, Copy)]
struct FirstKill {
    /// Our team's health fraction when the enemy healer died.
    our_hp_frac: f32,
    /// The off-target's health fraction at that instant.
    off_target_hp_frac: f32,
    /// Seconds between the healer's death and the end of the match.
    window: f32,
    /// How many of ours were still alive when the healer died.
    our_alive: usize,
    /// How many of ours died AFTER the healer did.
    our_deaths_after: usize,
    off_target_died: bool,
    won: bool,
}

fn run(map: &str, policy: &str, seed: u64) -> Option<FirstKill> {
    let mut at_kill: Option<(f32, f32, f32, usize)> = None;
    let mut alive_after_kill = 0usize;
    let mut last_t = 0.0f32;
    let mut off_died = false;

    let result = run_headless_match_observed(
        HeadlessMatchConfig {
            team1: vec!["Warlock".into(), "Priest".into()],
            team2: vec!["Priest".into(), "Rogue".into()],
            map: map.to_string(),
            team1_kill_target: Some(0),
            cc_policy: Some(policy.to_string()),
            max_duration_secs: 300.0,
            random_seed: Some(seed),
            ..Default::default()
        },
        true,
        None,
        |frame| {
            last_t = frame.sim_time;
            let healer_alive = frame
                .combatants
                .values()
                .any(|c| c.team == 2 && !c.is_pet && c.class == CharacterClass::Priest && c.alive);
            let off = frame
                .combatants
                .values()
                .find(|c| c.team == 2 && !c.is_pet && c.class == CharacterClass::Rogue);
            if let Some(o) = off {
                if !o.alive {
                    off_died = true;
                }
            }

            let ours: Vec<_> = frame.combatants.values().filter(|c| c.team == 1 && !c.is_pet).collect();
            let ours_alive = ours.iter().filter(|c| c.alive).count();

            if at_kill.is_none() && !healer_alive {
                // First frame with the called healer down.
                let cur: f32 = ours.iter().map(|c| c.current_health).sum();
                let max: f32 = ours.iter().map(|c| c.max_health).sum();
                at_kill = Some((
                    frame.sim_time,
                    cur / max.max(1.0),
                    off.map(|o| o.current_health / o.max_health.max(1.0)).unwrap_or(0.0),
                    ours_alive,
                ));
                alive_after_kill = ours_alive;
            }
        },
    )
    .ok()?;

    let (t, our_hp, off_hp, alive) = at_kill?;
    let ours_alive_at_end = result
        .team1_combatants
        .iter()
        .filter(|c| c.survived)
        .count();
    Some(FirstKill {
        our_hp_frac: our_hp,
        off_target_hp_frac: off_hp,
        window: (last_t - t).max(0.0),
        our_alive: alive,
        our_deaths_after: alive_after_kill.saturating_sub(ours_alive_at_end),
        off_target_died: off_died,
        won: result.winner == Some(1),
    })
}

fn cases() -> Vec<(&'static str, &'static str, u64)> {
    let mut v = Vec::new();
    for map in ["BasicArena", "PillaredArena"] {
        for policy in ["Identity", "Priced"] {
            for seed in 1..=8u64 {
                v.push((map, policy, seed));
            }
        }
    }
    v
}

fn mean(v: &[f32]) -> f32 {
    if v.is_empty() {
        return 0.0;
    }
    v.iter().sum::<f32>() / v.len() as f32
}

#[test]
fn the_probe_sees_first_kills() {
    let n = cases().iter().filter(|(m, p, s)| run(m, p, *s).is_some()).count();
    assert!(n > 0, "no match reached a first kill — the probe cannot test anything");
}

#[test]
#[ignore = "measurement: prints a table, asserts nothing. Run with --nocapture"]
fn report_state_at_first_kill() {
    let rows: Vec<FirstKill> = cases().iter().filter_map(|(m, p, s)| run(m, p, *s)).collect();
    let (won, lost): (Vec<&FirstKill>, Vec<&FirstKill>) = rows.iter().partition(|r| r.won);

    println!("\n=== State at the instant the called healer dies ===");
    println!("{} matches reached a first kill\n", rows.len());
    println!(
        "{:<10} {:>4} {:>14} {:>16} {:>10} {:>12} {:>16}",
        "result", "n", "our_hp_frac", "offtarget_hp", "window", "our_alive", "our_deaths_after"
    );
    for (label, set) in [("WON", &won), ("LOST", &lost)] {
        if set.is_empty() {
            continue;
        }
        println!(
            "{:<10} {:>4} {:>14.2} {:>16.2} {:>9.1}s {:>12.2} {:>16.2}",
            label,
            set.len(),
            mean(&set.iter().map(|r| r.our_hp_frac).collect::<Vec<_>>()),
            mean(&set.iter().map(|r| r.off_target_hp_frac).collect::<Vec<_>>()),
            mean(&set.iter().map(|r| r.window).collect::<Vec<_>>()),
            mean(&set.iter().map(|r| r.our_alive as f32).collect::<Vec<_>>()),
            mean(&set.iter().map(|r| r.our_deaths_after as f32).collect::<Vec<_>>()),
        );
    }

    println!("\nThe confound under test: if LOST shows our team already gutted at the");
    println!("first kill (low our_hp_frac, fewer alive), then a short window is a");
    println!("SYMPTOM of already losing and 'lengthen the window' is not a lever.");
    println!("If our_hp_frac is similar in both and only the window differs, the");
    println!("window is doing real work.");

    // How often does the off-target finish BOTH of ours after its healer is gone?
    let both = rows.iter().filter(|r| r.our_deaths_after >= 2).count();
    println!(
        "\nmatches where the off-target killed BOTH of ours after its healer died: {}/{}",
        both,
        rows.len()
    );
}
